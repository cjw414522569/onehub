//! T059 remote edit conflict detection and safe save: version fingerprints
//! prevent blind overwrites and preserve recovery copies.

use sftp_backend::{
    read_entire_file, FileAttrs, RemoteEditSession, SaveOutcome, SftpClient, SftpError, SftpServer,
    SftpStatus, SSH_FXF_CREAT, SSH_FXF_TRUNC, SSH_FXF_WRITE,
};
use tokio::io::duplex;

fn spawn_server(server_stream: tokio::io::DuplexStream) -> tokio::task::JoinHandle<SftpServer> {
    tokio::spawn(async move {
        let mut server = SftpServer::new();
        let mut stream = server_stream;
        server.serve(&mut stream).await.expect("serve");
        server
    })
}

#[tokio::test]
async fn save_without_change_succeeds() {
    let (client_stream, server_stream) = duplex(64 * 1024);
    let server_handle = spawn_server(server_stream);
    let mut client = SftpClient::new(client_stream);
    client.init().await.expect("init");
    client
        .open(
            "/doc.txt",
            SSH_FXF_WRITE | SSH_FXF_CREAT | SSH_FXF_TRUNC,
            &FileAttrs::default(),
        )
        .await
        .expect("seed");

    let (session, baseline) = RemoteEditSession::begin(&mut client, "/doc.txt")
        .await
        .expect("begin");
    assert_eq!(baseline.size, 0);

    let outcome = session.save(&mut client, b"my edit").await.expect("save");
    assert_eq!(outcome, SaveOutcome::Saved);
    assert_eq!(
        read_entire_file(&mut client, "/doc.txt")
            .await
            .expect("read"),
        b"my edit"
    );

    drop(client);
    let _server = server_handle.await.expect("server joined");
}

#[tokio::test]
async fn concurrent_modification_detects_conflict_and_keeps_recovery() {
    let (client_stream, server_stream) = duplex(64 * 1024);
    let server_handle = spawn_server(server_stream);
    let mut client = SftpClient::new(client_stream);
    client.init().await.expect("init");
    let seed_handle = client
        .open(
            "/doc.txt",
            SSH_FXF_WRITE | SSH_FXF_CREAT | SSH_FXF_TRUNC,
            &FileAttrs::default(),
        )
        .await
        .expect("seed");
    client
        .write(&seed_handle, 0, b"version-1")
        .await
        .expect("seed content");
    client.close(&seed_handle).await.expect("close seed");

    // Begin editing against version-1.
    let (session, baseline) = RemoteEditSession::begin(&mut client, "/doc.txt")
        .await
        .expect("begin");
    assert_eq!(baseline.size, 9);

    // A concurrent editor modifies the remote file between begin and save.
    let handle = client
        .open(
            "/doc.txt",
            SSH_FXF_WRITE | SSH_FXF_CREAT | SSH_FXF_TRUNC,
            &FileAttrs::default(),
        )
        .await
        .expect("open for concurrent edit");
    client
        .write(&handle, 0, b"version-2")
        .await
        .expect("concurrent write");
    client.close(&handle).await.expect("close concurrent");

    // Save with the stale baseline: must conflict, not overwrite.
    let outcome = session.save(&mut client, b"my edit").await.expect("save");
    let SaveOutcome::Conflict { recovery_path, .. } = outcome else {
        panic!("expected a conflict, got {outcome:?}");
    };
    // The remote still holds the concurrent editor's version.
    assert_eq!(
        read_entire_file(&mut client, "/doc.txt")
            .await
            .expect("read remote"),
        b"version-2",
        "remote must not be overwritten on conflict"
    );
    // The edited content was preserved as a recovery copy.
    let recovery = std::fs::read(&recovery_path).expect("recovery exists");
    assert_eq!(recovery, b"my edit");
    let _ = std::fs::remove_file(&recovery_path);

    drop(client);
    let _server = server_handle.await.expect("server joined");
}

#[tokio::test]
async fn begin_on_missing_file_reports_no_such_file() {
    let (client_stream, server_stream) = duplex(64 * 1024);
    let server_handle = spawn_server(server_stream);
    let mut client = SftpClient::new(client_stream);
    client.init().await.expect("init");
    let error = RemoteEditSession::begin(&mut client, "/missing.txt")
        .await
        .expect_err("must fail");
    assert_eq!(error, SftpError::Status(SftpStatus::NoSuchFile));
    drop(client);
    let _server = server_handle.await.expect("server joined");
}

#[tokio::test]
async fn version_fingerprint_tracks_content() {
    let (client_stream, server_stream) = duplex(64 * 1024);
    let server_handle = spawn_server(server_stream);
    let mut client = SftpClient::new(client_stream);
    client.init().await.expect("init");
    let handle = client
        .open(
            "/v.txt",
            SSH_FXF_WRITE | SSH_FXF_CREAT | SSH_FXF_TRUNC,
            &FileAttrs::default(),
        )
        .await
        .expect("seed");
    client.write(&handle, 0, b"alpha").await.expect("write");
    client.close(&handle).await.expect("close");
    let (_, v1) = RemoteEditSession::begin(&mut client, "/v.txt")
        .await
        .expect("begin");
    let handle = client
        .open(
            "/v.txt",
            SSH_FXF_WRITE | SSH_FXF_CREAT | SSH_FXF_TRUNC,
            &FileAttrs::default(),
        )
        .await
        .expect("open");
    client.write(&handle, 0, b"beta").await.expect("write");
    client.close(&handle).await.expect("close");
    let (_, v2) = RemoteEditSession::begin(&mut client, "/v.txt")
        .await
        .expect("begin v2");
    assert_ne!(
        v1.fingerprint, v2.fingerprint,
        "content change must change the fingerprint"
    );
    assert_eq!(v1.size, 5);
    assert_eq!(v2.size, 4);
    drop(client);
    let _server = server_handle.await.expect("server joined");
}
