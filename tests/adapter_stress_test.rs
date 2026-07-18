use std::time::Duration;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;

use tui_tetris::adapter::protocol::{create_hello, RequestedRole};
use tui_tetris::adapter::server::{build_observation, ServerConfig};
use tui_tetris::adapter::OutboundMessage;
use tui_tetris::core::GameState;

mod support;
use support::{read_json_line, spawn_server};

async fn hello(addr: std::net::SocketAddr, name: &str) -> TcpStream {
    let mut stream = TcpStream::connect(addr).await.unwrap();
    let mut hello = create_hello(1, name, "3.0.0");
    hello.requested.role = Some(RequestedRole::Observer);
    stream
        .write_all(serde_json::to_string(&hello).unwrap().as_bytes())
        .await
        .unwrap();
    stream.write_all(b"\n").await.unwrap();
    stream.flush().await.unwrap();
    stream
}

#[tokio::test]
async fn disconnect_storm_leaves_broker_responsive() {
    let config = ServerConfig {
        host: "127.0.0.1".into(),
        port: 0,
        protocol_version: "3.0.0".into(),
        ..ServerConfig::default()
    };
    let (server, addr, mut commands, _outbound) = spawn_server(config, 512).await;
    let drain = tokio::spawn(async move { while commands.recv().await.is_some() {} });

    for index in 0..200 {
        let stream = hello(addr, &format!("storm-{index}")).await;
        let (read, _) = stream.into_split();
        let mut lines = BufReader::new(read).lines();
        assert_eq!(read_json_line(&mut lines).await["type"], "welcome");
    }

    let stream = hello(addr, "healthy-after-storm").await;
    let (read, _) = stream.into_split();
    let mut lines = BufReader::new(read).lines();
    let welcome = read_json_line(&mut lines).await;
    assert_eq!(welcome["type"], "welcome");
    assert_eq!(welcome["role"], "observer");

    server.abort();
    drain.abort();
}

#[tokio::test]
async fn stalled_client_does_not_block_a_new_healthy_client() {
    let config = ServerConfig {
        host: "127.0.0.1".into(),
        port: 0,
        protocol_version: "3.0.0".into(),
        ..ServerConfig::default()
    };
    let (server, addr, mut commands, _outbound) = spawn_server(config, 512).await;
    let drain = tokio::spawn(async move { while commands.recv().await.is_some() {} });

    let slow = hello(addr, "slow").await;
    let (slow_read, mut slow_write) = slow.into_split();
    let mut slow_lines = BufReader::new(slow_read).lines();
    assert_eq!(read_json_line(&mut slow_lines).await["type"], "welcome");
    let flood = tokio::spawn(async move {
        for seq in 2..50_000u64 {
            let line = format!(
                "{{\"type\":\"command\",\"seq\":{seq},\"ts\":1,\"mode\":\"action\",\"actions\":[\"moveLeft\"]}}\n"
            );
            if slow_write.write_all(line.as_bytes()).await.is_err() {
                break;
            }
        }
    });

    let healthy = tokio::time::timeout(Duration::from_secs(2), hello(addr, "healthy"))
        .await
        .expect("healthy client connect blocked");
    let (read, _) = healthy.into_split();
    let mut lines = BufReader::new(read).lines();
    assert_eq!(read_json_line(&mut lines).await["type"], "welcome");

    flood.abort();
    server.abort();
    drain.abort();
}

#[tokio::test]
async fn latest_observation_fans_out_to_32_observers() {
    let config = ServerConfig {
        host: "127.0.0.1".into(),
        port: 0,
        protocol_version: "3.0.0".into(),
        ..ServerConfig::default()
    };
    let (server, addr, mut commands, outbound) = spawn_server(config, 512).await;
    let drain = tokio::spawn(async move { while commands.recv().await.is_some() {} });
    let mut clients = Vec::new();

    for index in 0..32 {
        let stream = hello(addr, &format!("observer-{index}")).await;
        let (read, write) = stream.into_split();
        let mut lines = BufReader::new(read).lines();
        assert_eq!(read_json_line(&mut lines).await["type"], "welcome");
        clients.push((lines, write));
    }

    let mut game = GameState::new(1);
    game.start();
    let observation = build_observation(77, 0, &game.snapshot(), &[]);
    outbound
        .send(OutboundMessage::BroadcastObservationArc {
            obs: std::sync::Arc::new(observation),
        })
        .unwrap();

    for (lines, _) in &mut clients {
        let observation = read_json_line(lines).await;
        assert_eq!(observation["type"], "observation");
        assert_eq!(observation["seq"], 77);
    }

    server.abort();
    drain.abort();
}
