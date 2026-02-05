use std::net::TcpListener;

use tui_tetris::adapter::server::check_tcp_listen_available;

#[test]
fn adapter_port_check_fails_when_port_in_use() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind ephemeral port");
    let port = listener.local_addr().unwrap().port();

    let err = check_tcp_listen_available("127.0.0.1", port).expect_err("expected addr in use");
    assert_eq!(err.kind(), std::io::ErrorKind::AddrInUse);
}

