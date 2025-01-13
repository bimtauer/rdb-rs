use assert_cmd::Command;
use pretty_assertions::assert_eq;
use rdb::{self, filter, formatter};
use redis::Client;
use rstest::rstest;
use std::fs;
use std::fs::File;
use std::io::BufReader;
use std::os::unix::fs::MetadataExt;
use std::path::Path;
use std::path::PathBuf;
use tempfile::tempdir;
use tempfile::TempDir;
use testcontainers::core::ExecCommand;
use testcontainers::core::Mount;
use testcontainers::ContainerAsync;
use testcontainers_modules::{
    redis::Redis, testcontainers::runners::AsyncRunner, testcontainers::ImageExt,
};

fn run_dump_test(input: PathBuf, format: &str) -> String {
    let file_stem = input
        .file_stem()
        .expect("File should have a name")
        .to_string_lossy();
    let temp_output = PathBuf::from(format!("/tmp/rdb_test_{}.{}", file_stem, format));

    let file = fs::File::open(&input).expect("Failed to open dump file");
    let reader = BufReader::new(file);
    match format {
        "json" => {
            let formatter = formatter::JSON::new(Some(temp_output.clone()));
            let filter = filter::Simple::new();
            rdb::parse(reader, formatter, filter).expect("Failed to parse RDB file");
        }
        "protocol" => {
            let formatter = formatter::Protocol::new(Some(temp_output.clone()));
            let filter = filter::Simple::new();
            rdb::parse(reader, formatter, filter).expect("Failed to parse RDB file");
        }
        "plain" => {
            let formatter = formatter::Plain::new(Some(temp_output.clone()));
            let filter = filter::Simple::new();
            rdb::parse(reader, formatter, filter).expect("Failed to parse RDB file");
        }
        _ => {
            panic!("Invalid format: {}", format);
        }
    }

    let actual =
        String::from_utf8_lossy(&fs::read(&temp_output).expect("Failed to read output file"))
            .into_owned();

    fs::remove_file(&temp_output).ok();

    actual
}

fn load_expected(path: PathBuf, format: &str) -> String {
    let file_stem = path
        .file_stem()
        .expect("File should have a name")
        .to_string_lossy();
    let expected_json_path = format!("tests/dumps/{}/{}.{}", format, file_stem, format);
    fs::read_to_string(&expected_json_path).unwrap_or_else(|_| {
        String::from_utf8_lossy(&fs::read(&expected_json_path).expect("Failed to read file"))
            .into_owned()
    })
}

#[rstest]
#[case::json("json")]
#[case::plain("plain")]
#[case::protocol("protocol")]
fn test_dump_matches_expected(#[files("tests/dumps/*.rdb")] path: PathBuf, #[case] format: &str) {
    let actual = run_dump_test(path.clone(), format);

    let expected = load_expected(path.clone(), format);

    assert_eq!(
        actual,
        expected,
        "Output doesn't match for {}",
        path.display()
    );
}

async fn redis_client(
    major_version: u8,
    minor_version: u8,
) -> (Client, TempDir, ContainerAsync<Redis>) {
    let tmp_dir = tempdir().unwrap();
    let container = Redis::default()
        .with_tag(format!("{}.{}-alpine", major_version, minor_version))
        .with_mount(Mount::bind_mount(
            tmp_dir.path().display().to_string(),
            "/data",
        ))
        //.with_cmd(vec!["/bin/sh", "-c", "chown -R 1001:1001 /data && redis-server --user 1001:1001"])
        .start()
        .await
        .expect("Failed to start Redis container");

    let mut debug_cmd = container.exec(ExecCommand::new(["id"])).await.unwrap();
    println!(
        "Container running as: {}",
        String::from_utf8_lossy(&debug_cmd.stdout_to_vec().await.unwrap())
    );

    let host_ip = container.get_host().await.unwrap();
    let host_port = container.get_host_port_ipv4(6379).await.unwrap();
    let url = format!("redis://{}:{}", host_ip, host_port);
    let client = Client::open(url).expect("Failed to create Redis client");

    (client, tmp_dir, container)
}

fn to_resp_format(command: &str, args: &[&str]) -> String {
    let mut resp = format!("*{}\r\n", args.len() + 1); // +1 for command itself
    resp.push_str(&format!("${}\r\n{}\r\n", command.len(), command));
    for arg in args {
        resp.push_str(&format!("${}\r\n{}\r\n", arg.len(), arg));
    }
    resp
}

async fn execute_commands(conn: &mut redis::Connection, commands: &[(&str, Vec<&str>)]) -> String {
    let mut resp = String::from("*2\r\n$6\r\nSELECT\r\n$1\r\n0\r\n"); // Initial SELECT command

    for (cmd, args) in commands {
        // Execute the command
        let mut redis_cmd = redis::cmd(cmd);
        for arg in args {
            redis_cmd.arg(arg);
        }
        redis_cmd.exec(conn).unwrap();

        // Add RESP representation
        resp.push_str(&to_resp_format(cmd, args));
    }

    resp
}

fn parse_rdb_to_resp(rdb_path: &Path) -> String {
    let rdb_file = File::open(rdb_path).unwrap();
    let rdb_reader = BufReader::new(rdb_file);
    let tmp_file = tempfile::NamedTempFile::new().unwrap();

    rdb::parse(
        rdb_reader,
        rdb::formatter::Protocol::new(Some(tmp_file.path().to_path_buf())),
        rdb::filter::Simple::new(),
    )
    .unwrap();

    let output = std::fs::read(tmp_file.path()).unwrap();

    String::from_utf8(output).unwrap()
}

fn split_resp_commands(resp: &str) -> Vec<String> {
    // Skip the initial SELECT command
    let mut commands: Vec<String> = resp
        .split("*")
        .filter(|s| !s.is_empty())
        .map(|s| format!("*{}", s))
        .collect();

    // Remove the SELECT command if it exists
    if !commands.is_empty() && commands[0].contains("SELECT") {
        commands.remove(0);
    }

    commands
}

#[link(name = "c")]
unsafe extern "C" {
    unsafe fn geteuid() -> u32;
    unsafe fn getegid() -> u32;
}

#[rstest]
#[case::redis_6_2(6, 2)]
#[case::redis_7_0(7, 0)]
#[case::redis_7_2(7, 2)]
#[case::redis_7_4(7, 4)]
#[tokio::test]
async fn test_redis_protocol_reproducibility(#[case] major_version: u8, #[case] minor_version: u8) {
    use testcontainers::core::ExecCommand;

    let (client, tmp_dir, container) = redis_client(major_version, minor_version).await;
    let mut conn = client.get_connection().unwrap();

    let commands = vec![
        ("SET", vec!["string", "bar"]),
        ("HSET", vec!["hash", "name", "John"]),
        ("HSET", vec!["hash", "age", "25"]),
        ("SADD", vec!["set_strings", "Earth"]),
        ("SADD", vec!["set_strings", "Mars"]),
        ("SADD", vec!["set_strings", "Jupiter"]),
        ("SADD", vec!["set_numbers", "1"]),
        ("SADD", vec!["set_numbers", "2"]),
        ("SADD", vec!["set_numbers", "3"]),
        ("ZADD", vec!["sorted_set", "1", "a"]),
        ("ZADD", vec!["sorted_set", "2", "b"]),
        ("ZADD", vec!["sorted_set", "3", "c"]),
        ("RPUSH", vec!["list_strings", "Rex"]),
        ("RPUSH", vec!["list_strings", "Bella"]),
        ("RPUSH", vec!["list_strings", "Max"]),
        ("RPUSH", vec!["list_numbers", "1"]),
        ("RPUSH", vec!["list_numbers", "2"]),
        ("RPUSH", vec!["list_numbers", "3"]),
    ];

    let expected_resp = execute_commands(&mut conn, &commands).await;
    redis::cmd("SAVE").exec(&mut conn).unwrap();

    // Give Redis a moment to finish writing
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    let rdb_file = Path::new(&tmp_dir.path()).join("dump.rdb");

    let mut user_in_container = container.exec(ExecCommand::new(["id"])).await.unwrap();

    let mut group_in_container = container
        .exec(ExecCommand::new(["id", "-g"]))
        .await
        .unwrap();
    println!(
        "User in container: {}:{}",
        String::from_utf8_lossy(&user_in_container.stdout_to_vec().await.unwrap()),
        String::from_utf8_lossy(&group_in_container.stdout_to_vec().await.unwrap())
    );

    // Debug before chmod/chown
    let mut ls_before = container
        .exec(ExecCommand::new(["ls", "-l", "/data/dump.rdb"]))
        .await
        .unwrap();
    println!(
        "Before chmod/chown: {}",
        String::from_utf8_lossy(&ls_before.stdout_to_vec().await.unwrap())
    );

    container
        .exec(ExecCommand::new(["chmod", "644", "/data/dump.rdb"]))
        .await
        .unwrap();
    container
        .exec(ExecCommand::new(["chown", "1001:121", "/data/dump.rdb"]))
        .await
        .unwrap();

    // Debug after chmod/chown
    let mut ls_after = container
        .exec(ExecCommand::new(["ls", "-l", "/data/dump.rdb"]))
        .await
        .unwrap();
    println!(
        "After chmod/chown: {}",
        String::from_utf8_lossy(&ls_after.stdout_to_vec().await.unwrap())
    );

    let metadata = rdb_file.metadata().unwrap();
    let mode = metadata.mode() & 0o777; // Apply mask to get just permission bits
    println!(
        "Path: {:?}, File owner: {:?}, permissions: {:04o} ({})",
        rdb_file,
        metadata.uid(),
        mode,
        format_mode(mode)
    );
    println!("Running as user inside test: {}:{}", unsafe { geteuid() }, unsafe { getegid() });
    let actual_resp = parse_rdb_to_resp(&rdb_file);

    // Compare commands as unordered sets
    let expected_commands: std::collections::HashSet<_> =
        split_resp_commands(&expected_resp).into_iter().collect();
    let actual_commands: std::collections::HashSet<_> =
        split_resp_commands(&actual_resp).into_iter().collect();

    assert_eq!(actual_commands, expected_commands);
    assert!(false);
}

#[rstest]
fn test_cli_commands_succeed(
    #[files("tests/dumps/*.rdb")] path: PathBuf,
    #[values("json", "plain", "protocol")] format: &str,
    #[values("", "1")] databases: &str,
    #[values("", "hash", "set", "list", "sortedset", "string")] types: &str,
) {
    let mut cmd = Command::cargo_bin("rdb").unwrap();

    cmd.args(["--format", format]);

    if !databases.is_empty() {
        cmd.args(["--databases", databases]);
    }

    if !types.is_empty() {
        cmd.args(["--type", types]);
    }

    cmd.arg(&path).assert().success();
}

fn format_mode(mode: u32) -> String {
    let user = [(mode >> 6) & 0o7];
    let group = [(mode >> 3) & 0o7];
    let other = [mode & 0o7];

    let convert = |bits: [u32; 1]| {
        let mut s = String::with_capacity(3);
        s.push(if bits[0] & 0o4 != 0 { 'r' } else { '-' });
        s.push(if bits[0] & 0o2 != 0 { 'w' } else { '-' });
        s.push(if bits[0] & 0o1 != 0 { 'x' } else { '-' });
        s
    };

    format!("{}{}{}", convert(user), convert(group), convert(other))
}
