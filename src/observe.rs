use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use anyhow::{anyhow, Result};

use crate::adapter::protocol::{
    create_hello, CommandMode, ObservationMessage, PieceKindLower, RequestedRole, RotationLower,
};
use crate::core::snapshot::{ActiveSnapshot, GameSnapshot, TimersSnapshot};
use crate::types::{PieceKind, Rotation, BOARD_HEIGHT, BOARD_WIDTH};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObserveConfig {
    pub host: String,
    pub port: u16,
}

#[derive(Debug, Clone)]
pub enum ObserveEvent {
    Welcome,
    Observation(ObservationMessage),
    Error(String),
    Closed,
}

pub fn parse_observe_args(args: &[String]) -> Result<Option<ObserveConfig>> {
    if args.is_empty() || args[0] != "observe" {
        return Ok(None);
    }

    let mut host = String::from("127.0.0.1");
    let mut port: u16 = 7777;
    let mut i = 1usize;
    while i < args.len() {
        match args[i].as_str() {
            "--host" => {
                i += 1;
                let v = args
                    .get(i)
                    .ok_or_else(|| anyhow!("observe: missing value for --host"))?;
                host = v.clone();
            }
            "--port" => {
                i += 1;
                let v = args
                    .get(i)
                    .ok_or_else(|| anyhow!("observe: missing value for --port"))?;
                port = v
                    .parse::<u16>()
                    .map_err(|_| anyhow!("observe: invalid --port value: {}", v))?;
            }
            other => {
                return Err(anyhow!("observe: unknown argument: {}", other));
            }
        }
        i += 1;
    }

    Ok(Some(ObserveConfig { host, port }))
}

pub fn connect_observer(config: &ObserveConfig) -> Result<mpsc::Receiver<ObserveEvent>> {
    let mut stream = TcpStream::connect((config.host.as_str(), config.port))
        .map_err(|e| anyhow!("observe: connect {}:{} failed: {}", config.host, config.port, e))?;
    stream
        .set_nodelay(true)
        .map_err(|e| anyhow!("observe: set_nodelay failed: {}", e))?;

    let mut hello = create_hello(1, "tui-tetris-observe", "2.0.0");
    hello.requested.stream_observations = true;
    hello.requested.command_mode = CommandMode::Action;
    hello.requested.role = Some(RequestedRole::Observer);
    let line = serde_json::to_string(&hello)?;
    stream.write_all(line.as_bytes())?;
    stream.write_all(b"\n")?;
    stream.flush()?;

    let (tx, rx) = mpsc::channel::<ObserveEvent>();
    thread::spawn(move || {
        let reader = BufReader::new(stream);
        for line in reader.lines() {
            let line = match line {
                Ok(line) => line,
                Err(e) => {
                    let _ = tx.send(ObserveEvent::Error(format!("observe: read error: {}", e)));
                    let _ = tx.send(ObserveEvent::Closed);
                    return;
                }
            };
            if line.trim().is_empty() {
                continue;
            }
            if let Some(event) = parse_server_line(&line) {
                let _ = tx.send(event);
            }
        }
        let _ = tx.send(ObserveEvent::Closed);
    });

    Ok(rx)
}

pub fn observe_status_lines(
    config: &ObserveConfig,
    obs: Option<&ObservationMessage>,
) -> [String; 5] {
    let (state, ep, piece, step, seed) = match obs {
        Some(o) => {
            let state = if o.game_over {
                "GAME_OVER"
            } else if o.paused {
                "PAUSED"
            } else if o.playable {
                "PLAY"
            } else {
                "IDLE"
            };
            (
                state.to_string(),
                o.episode_id.to_string(),
                o.piece_id.to_string(),
                o.step_in_piece.to_string(),
                o.seed.to_string(),
            )
        }
        None => (
            "WAITING".to_string(),
            "-".to_string(),
            "-".to_string(),
            "-".to_string(),
            "-".to_string(),
        ),
    };

    [
        "MODE OBSERVE".to_string(),
        format!("TARGET {}:{}", config.host, config.port),
        format!("STATE {}", state),
        format!("EP {} PIECE {} STEP {}", ep, piece, step),
        format!("SEED {}", seed),
    ]
}

pub fn snapshot_from_observation(obs: &ObservationMessage) -> GameSnapshot {
    let mut board = [[0u8; BOARD_WIDTH as usize]; BOARD_HEIGHT as usize];
    board.copy_from_slice(&obs.board.cells);

    let active = obs.active.as_ref().map(|a| ActiveSnapshot {
        kind: piece_kind_from_lower(a.kind),
        rotation: rotation_from_lower(a.rotation),
        x: a.x,
        y: a.y,
    });

    let mut next_queue = [PieceKind::I; 5];
    for (i, k) in obs.next_queue.iter().copied().enumerate() {
        next_queue[i] = piece_kind_from_lower(k);
    }

    GameSnapshot {
        board,
        board_id: obs.board_id,
        board_hash: 0,
        active,
        ghost_y: obs.ghost_y,
        hold: obs.hold.map(piece_kind_from_lower),
        next_queue,
        can_hold: obs.can_hold,
        paused: obs.paused,
        game_over: obs.game_over,
        episode_id: obs.episode_id,
        seed: obs.seed,
        piece_id: obs.piece_id,
        step_in_piece: obs.step_in_piece,
        score: obs.score,
        level: obs.level,
        lines: obs.lines,
        timers: TimersSnapshot {
            drop_ms: obs.timers.drop_ms,
            lock_ms: obs.timers.lock_ms,
            line_clear_ms: obs.timers.line_clear_ms,
        },
    }
}

fn piece_kind_from_lower(value: PieceKindLower) -> PieceKind {
    match value {
        PieceKindLower::I => PieceKind::I,
        PieceKindLower::O => PieceKind::O,
        PieceKindLower::T => PieceKind::T,
        PieceKindLower::S => PieceKind::S,
        PieceKindLower::Z => PieceKind::Z,
        PieceKindLower::J => PieceKind::J,
        PieceKindLower::L => PieceKind::L,
    }
}

fn rotation_from_lower(value: RotationLower) -> Rotation {
    match value {
        RotationLower::North => Rotation::North,
        RotationLower::East => Rotation::East,
        RotationLower::South => Rotation::South,
        RotationLower::West => Rotation::West,
    }
}

pub fn wait_for_welcome(
    rx: &mpsc::Receiver<ObserveEvent>,
    timeout: Duration,
) -> Result<Option<ObservationMessage>> {
    let deadline = std::time::Instant::now() + timeout;
    let mut got_welcome = false;
    let mut first_obs: Option<ObservationMessage> = None;

    while std::time::Instant::now() < deadline {
        match rx.recv_timeout(Duration::from_millis(50)) {
            Ok(ObserveEvent::Welcome) => {
                got_welcome = true;
                if first_obs.is_some() {
                    break;
                }
            }
            Ok(ObserveEvent::Observation(obs)) => {
                first_obs = Some(obs);
                if got_welcome {
                    break;
                }
            }
            Ok(ObserveEvent::Error(msg)) => return Err(anyhow!(msg)),
            Ok(ObserveEvent::Closed) => return Err(anyhow!("observe: connection closed")),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                return Err(anyhow!("observe: event channel disconnected"));
            }
        }
    }

    if !got_welcome {
        return Err(anyhow!("observe: did not receive welcome"));
    }
    Ok(first_obs)
}

fn parse_server_line(line: &str) -> Option<ObserveEvent> {
    let value: serde_json::Value = match serde_json::from_str(line) {
        Ok(v) => v,
        Err(e) => return Some(ObserveEvent::Error(format!("observe: invalid json: {}", e))),
    };
    let msg_type = value.get("type").and_then(|v| v.as_str()).unwrap_or("");
    match msg_type {
        "welcome" => Some(ObserveEvent::Welcome),
        "observation" => match serde_json::from_str::<ObservationMessage>(line) {
            Ok(obs) => Some(ObserveEvent::Observation(obs)),
            Err(e) => Some(ObserveEvent::Error(format!(
                "observe: invalid observation: {}",
                e
            ))),
        },
        "error" => {
            let code = value.get("code").and_then(|v| v.as_str()).unwrap_or("unknown");
            let msg = value.get("message").and_then(|v| v.as_str()).unwrap_or("");
            Some(ObserveEvent::Error(format!(
                "observe: server error {} {}",
                code, msg
            )))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapter::protocol::{
        ActivePieceSnapshot, BoardSnapshot, ObservationType, PieceKindLower, StateHash,
    };

    #[test]
    fn parse_observe_args_parses_host_port() {
        let args = vec![
            "observe".to_string(),
            "--host".to_string(),
            "0.0.0.0".to_string(),
            "--port".to_string(),
            "9001".to_string(),
        ];
        let cfg = parse_observe_args(&args).unwrap().unwrap();
        assert_eq!(
            cfg,
            ObserveConfig {
                host: "0.0.0.0".to_string(),
                port: 9001
            }
        );
    }

    #[test]
    fn parse_observe_args_uses_defaults() {
        let args = vec!["observe".to_string()];
        let cfg = parse_observe_args(&args).unwrap().unwrap();
        assert_eq!(
            cfg,
            ObserveConfig {
                host: "127.0.0.1".to_string(),
                port: 7777
            }
        );
    }

    #[test]
    fn snapshot_from_observation_maps_fields() {
        let obs = ObservationMessage {
            msg_type: ObservationType::Observation,
            seq: 2,
            ts: 1,
            playable: true,
            paused: false,
            game_over: false,
            episode_id: 7,
            seed: 123,
            piece_id: 9,
            step_in_piece: 1,
            board: BoardSnapshot {
                width: 10,
                height: 20,
                cells: [[0u8; 10]; 20],
            },
            board_id: 10,
            active: Some(ActivePieceSnapshot {
                kind: PieceKindLower::T,
                rotation: RotationLower::East,
                x: 4,
                y: 2,
            }),
            ghost_y: Some(18),
            next: PieceKindLower::I,
            next_queue: [
                PieceKindLower::I,
                PieceKindLower::O,
                PieceKindLower::T,
                PieceKindLower::S,
                PieceKindLower::Z,
            ],
            hold: Some(PieceKindLower::L),
            can_hold: true,
            last_event: None,
            state_hash: StateHash(1),
            score: 300,
            level: 2,
            lines: 4,
            timers: crate::adapter::protocol::TimersSnapshot {
                drop_ms: 1000,
                lock_ms: 500,
                line_clear_ms: 0,
            },
        };

        let snap = snapshot_from_observation(&obs);
        assert_eq!(snap.board_id, 10);
        assert_eq!(snap.episode_id, 7);
        assert_eq!(snap.seed, 123);
        assert_eq!(snap.piece_id, 9);
        assert_eq!(snap.step_in_piece, 1);
        assert_eq!(snap.score, 300);
        assert_eq!(snap.level, 2);
        assert_eq!(snap.lines, 4);
        assert_eq!(snap.hold, Some(PieceKind::L));
        assert_eq!(snap.next_queue[0], PieceKind::I);
        assert_eq!(snap.next_queue[1], PieceKind::O);
        assert!(snap.active.is_some());
        let active = snap.active.unwrap();
        assert_eq!(active.kind, PieceKind::T);
        assert_eq!(active.rotation, Rotation::East);
        assert_eq!(active.x, 4);
        assert_eq!(active.y, 2);
    }

    #[test]
    fn parse_server_line_accepts_observation_rotation_strings() {
        let line = r#"{"type":"observation","seq":1,"ts":1,"playable":true,"paused":false,"game_over":false,"episode_id":1,"seed":1,"piece_id":1,"step_in_piece":0,"board":{"width":10,"height":20,"cells":[[0,0,0,0,0,0,0,0,0,0],[0,0,0,0,0,0,0,0,0,0],[0,0,0,0,0,0,0,0,0,0],[0,0,0,0,0,0,0,0,0,0],[0,0,0,0,0,0,0,0,0,0],[0,0,0,0,0,0,0,0,0,0],[0,0,0,0,0,0,0,0,0,0],[0,0,0,0,0,0,0,0,0,0],[0,0,0,0,0,0,0,0,0,0],[0,0,0,0,0,0,0,0,0,0],[0,0,0,0,0,0,0,0,0,0],[0,0,0,0,0,0,0,0,0,0],[0,0,0,0,0,0,0,0,0,0],[0,0,0,0,0,0,0,0,0,0],[0,0,0,0,0,0,0,0,0,0],[0,0,0,0,0,0,0,0,0,0],[0,0,0,0,0,0,0,0,0,0],[0,0,0,0,0,0,0,0,0,0],[0,0,0,0,0,0,0,0,0,0],[0,0,0,0,0,0,0,0,0,0]]},"board_id":1,"active":{"kind":"t","rotation":"north","x":4,"y":0},"ghost_y":18,"next":"i","next_queue":["i","o","t","s","z"],"hold":null,"can_hold":true,"state_hash":"0000000000000001","score":0,"level":0,"lines":0,"timers":{"drop_ms":1000,"lock_ms":500,"line_clear_ms":0}}"#;
        let event = parse_server_line(line).expect("event");
        match event {
            ObserveEvent::Observation(obs) => {
                assert_eq!(obs.active.unwrap().rotation, RotationLower::North);
            }
            _ => panic!("expected observation"),
        }
    }

    #[test]
    fn observe_status_lines_include_mode_target_and_episode_fields() {
        let cfg = ObserveConfig {
            host: "127.0.0.1".to_string(),
            port: 7780,
        };
        let obs = ObservationMessage {
            msg_type: ObservationType::Observation,
            seq: 2,
            ts: 1,
            playable: true,
            paused: false,
            game_over: false,
            episode_id: 7,
            seed: 123,
            piece_id: 9,
            step_in_piece: 1,
            board: BoardSnapshot {
                width: 10,
                height: 20,
                cells: [[0u8; 10]; 20],
            },
            board_id: 10,
            active: None,
            ghost_y: None,
            next: PieceKindLower::I,
            next_queue: [
                PieceKindLower::I,
                PieceKindLower::O,
                PieceKindLower::T,
                PieceKindLower::S,
                PieceKindLower::Z,
            ],
            hold: None,
            can_hold: true,
            last_event: None,
            state_hash: StateHash(1),
            score: 0,
            level: 0,
            lines: 0,
            timers: crate::adapter::protocol::TimersSnapshot {
                drop_ms: 1000,
                lock_ms: 500,
                line_clear_ms: 0,
            },
        };

        let lines = observe_status_lines(&cfg, Some(&obs));
        assert_eq!(lines[0], "MODE OBSERVE");
        assert_eq!(lines[1], "TARGET 127.0.0.1:7780");
        assert_eq!(lines[2], "STATE PLAY");
        assert_eq!(lines[3], "EP 7 PIECE 9 STEP 1");
        assert_eq!(lines[4], "SEED 123");
    }
}
