//! T056 SFTP v3 integration tests: client <-> in-memory server over a duplex
//! stream. Real OpenSSH SFTP is `blocked_environment` on this host (no sshd);
//! the wire format matches the OpenSSH SFTP v3 protocol.

use sftp_backend::{
    FileAttrs, SftpClient, SftpError, SftpServer, SftpStatus, SSH_FXF_CREAT, SSH_FXF_READ,
    SSH_FXF_TRUNC, SSH_FXF_WRITE,
};
use tokio::io::duplex;

/// Runs a server task over one duplex half; returns the server after the
/// client disconnects.
async fn spawn_server(
    server_stream: tokio::io::DuplexStream,
) -> tokio::task::JoinHandle<SftpServer> {
    tokio::spawn(async move {
        let mut server = SftpServer::new();
        let mut stream = server_stream;
        server.serve(&mut stream).await.expect("serve");
        server
    })
}

#[tokio::test]
async fn init_probes_version_and_capabilities() {
    let (client_stream, server_stream) = duplex(64 * 1024);
    let server_handle = spawn_server(server_stream).await;
    let mut client = SftpClient::new(client_stream);
    let capabilities = client.init().await.expect("init");
    assert_eq!(capabilities.version, 3);
    assert!(capabilities.supports("posix-rename@openssh.com"));
    assert!(capabilities.supports("statvfs@openssh.com"));
    assert!(!capabilities.supports("nope@example.com"));
    drop(client);
    let server = server_handle.await.expect("server joined");
    assert!(server.fs().lookup("/").is_ok());
}

#[tokio::test]
async fn mkdir_list_and_stat() {
    let (client_stream, server_stream) = duplex(64 * 1024);
    let server_handle = spawn_server(server_stream).await;
    let mut client = SftpClient::new(client_stream);
    client.init().await.expect("init");

    client
        .mkdir("/data", &FileAttrs::directory(0o755))
        .await
        .expect("mkdir");
    let root = client.opendir("/").await.expect("opendir");
    let entries = client.readdir(&root).await.expect("readdir");
    assert!(
        entries.iter().any(|(name, _, _)| name == "data"),
        "root listing must contain data"
    );
    let attrs = client.lstat("/data").await.expect("lstat");
    assert!(attrs.is_dir());
    assert_eq!(attrs.mode_string(), "drwxr-xr-x");

    // Missing path -> NoSuchFile.
    let error = client.lstat("/missing").await.expect_err("missing");
    assert_eq!(error, SftpError::Status(SftpStatus::NoSuchFile));
    drop(client);
    let _server = server_handle.await.expect("server joined");
}

#[tokio::test]
async fn write_read_fstat_and_truncate() {
    let (client_stream, server_stream) = duplex(64 * 1024);
    let server_handle = spawn_server(server_stream).await;
    let mut client = SftpClient::new(client_stream);
    client.init().await.expect("init");

    let handle = client
        .open(
            "/file.txt",
            SSH_FXF_WRITE | SSH_FXF_CREAT | SSH_FXF_TRUNC,
            &FileAttrs::default(),
        )
        .await
        .expect("open write");
    client
        .write(&handle, 0, b"hello world")
        .await
        .expect("write");
    client.write(&handle, 6, b"rust").await.expect("write mid");
    client.close(&handle).await.expect("close");

    let handle = client
        .open("/file.txt", SSH_FXF_READ, &FileAttrs::default())
        .await
        .expect("open read");
    let data = client.read(&handle, 0, 100).await.expect("read");
    // "world" was created; "rust" overwrote offset 6..10, leaving the trailing
    // 'd' (write does not truncate), matching POSIX write semantics.
    assert_eq!(data, b"hello rustd");
    let eof = client.read(&handle, 100, 10).await.expect("read eof");
    assert!(eof.is_empty(), "read past EOF must yield empty");
    let attrs = client.fstat(&handle).await.expect("fstat");
    assert_eq!(attrs.size, Some(11));
    assert!(attrs.is_regular());
    client.close(&handle).await.expect("close");

    // Opening with TRUNC resets the file.
    let handle = client
        .open(
            "/file.txt",
            SSH_FXF_WRITE | SSH_FXF_TRUNC,
            &FileAttrs::default(),
        )
        .await
        .expect("open trunc");
    client.close(&handle).await.expect("close");
    let handle = client
        .open("/file.txt", SSH_FXF_READ, &FileAttrs::default())
        .await
        .expect("open read");
    let data = client.read(&handle, 0, 100).await.expect("read");
    assert!(data.is_empty(), "truncated file must be empty");
    drop(client);
    let _server = server_handle.await.expect("server joined");
}

#[tokio::test]
async fn rename_delete_and_status_codes() {
    let (client_stream, server_stream) = duplex(64 * 1024);
    let server_handle = spawn_server(server_stream).await;
    let mut client = SftpClient::new(client_stream);
    client.init().await.expect("init");

    client
        .open(
            "/a.txt",
            SSH_FXF_WRITE | SSH_FXF_CREAT,
            &FileAttrs::default(),
        )
        .await
        .expect("create a");
    client.rename("/a.txt", "/b.txt").await.expect("rename");
    assert!(client.stat("/b.txt").await.expect("stat b").is_regular());
    assert!(matches!(
        client.stat("/a.txt").await,
        Err(SftpError::Status(SftpStatus::NoSuchFile))
    ));

    client.remove("/b.txt").await.expect("remove");
    assert!(matches!(
        client.stat("/b.txt").await,
        Err(SftpError::Status(SftpStatus::NoSuchFile))
    ));
    assert!(matches!(
        client.remove("/b.txt").await,
        Err(SftpError::Status(SftpStatus::NoSuchFile))
    ));

    // Removing a directory with remove() is a failure.
    client
        .mkdir("/dir", &FileAttrs::directory(0o755))
        .await
        .expect("mkdir");
    assert!(matches!(
        client.remove("/dir").await,
        Err(SftpError::Status(SftpStatus::Failure))
    ));
    client.rmdir("/dir").await.expect("rmdir");

    // posix-rename@openssh.com extension.
    client
        .open(
            "/p.txt",
            SSH_FXF_WRITE | SSH_FXF_CREAT,
            &FileAttrs::default(),
        )
        .await
        .expect("create p");
    let mut data = Vec::new();
    data.extend_from_slice(&("/p.txt".len() as u32).to_be_bytes());
    data.extend_from_slice(b"/p.txt");
    data.extend_from_slice(&("/q.txt".len() as u32).to_be_bytes());
    data.extend_from_slice(b"/q.txt");
    client
        .extended("posix-rename@openssh.com", &data)
        .await
        .expect("posix-rename");
    assert!(client.stat("/q.txt").await.expect("stat q").is_regular());
    drop(client);
    let _server = server_handle.await.expect("server joined");
}

#[tokio::test]
async fn permissions_and_symlinks() {
    let (client_stream, server_stream) = duplex(64 * 1024);
    let server_handle = spawn_server(server_stream).await;
    let mut client = SftpClient::new(client_stream);
    client.init().await.expect("init");

    client
        .mkdir("/secure", &FileAttrs::directory(0o700))
        .await
        .expect("mkdir");
    let attrs = client.stat("/secure").await.expect("stat");
    assert!(attrs.is_dir());
    assert_eq!(attrs.permission_bits() & 0o777, 0o700);
    assert_eq!(attrs.mode_string(), "drwx------");

    client.symlink("/link", "/secure").await.expect("symlink");
    let target = client.readlink("/link").await.expect("readlink");
    assert_eq!(target, "/secure");
    let lstat = client.lstat("/link").await.expect("lstat");
    assert!(lstat.is_symlink(), "lstat must not follow the link");
    let stat = client.stat("/link").await.expect("stat");
    assert!(stat.is_dir(), "stat must follow the link to the directory");

    // setstat updates permissions.
    client
        .setstat(
            "/secure",
            &FileAttrs {
                permissions: Some(0o755),
                ..FileAttrs::default()
            },
        )
        .await
        .expect("setstat");
    let attrs = client.stat("/secure").await.expect("stat after setstat");
    assert_eq!(attrs.permission_bits() & 0o777, 0o755);
    drop(client);
    let _server = server_handle.await.expect("server joined");
}

#[tokio::test]
async fn realpath_and_nested_directories() {
    let (client_stream, server_stream) = duplex(64 * 1024);
    let server_handle = spawn_server(server_stream).await;
    let mut client = SftpClient::new(client_stream);
    client.init().await.expect("init");

    client
        .mkdir("/a", &FileAttrs::directory(0o755))
        .await
        .expect("mkdir a");
    client
        .mkdir("/a/b", &FileAttrs::directory(0o755))
        .await
        .expect("mkdir b");
    let canonical = client.realpath("/a/b/../b").await.expect("realpath");
    assert_eq!(canonical, "/a/b");

    // Directory listing of /a contains b.
    let dir = client.opendir("/a").await.expect("opendir");
    let entries = client.readdir(&dir).await.expect("readdir");
    assert!(entries
        .iter()
        .any(|(name, _, attrs)| name == "b" && attrs.is_dir()));
    drop(client);
    let _server = server_handle.await.expect("server joined");
}

#[tokio::test]
async fn unsupported_extension_is_reported() {
    let (client_stream, server_stream) = duplex(64 * 1024);
    let server_handle = spawn_server(server_stream).await;
    let mut client = SftpClient::new(client_stream);
    client.init().await.expect("init");
    let error = client
        .extended("unknown@example.com", &[])
        .await
        .expect_err("unknown extension");
    assert_eq!(error, SftpError::Status(SftpStatus::Unsupported));
    drop(client);
    let _server = server_handle.await.expect("server joined");
}
