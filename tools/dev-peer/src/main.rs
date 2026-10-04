//! `dev-peer --server http://127.0.0.1:8787 --role invite|accept --mine LAT,LON --expect LAT,LON`
//! Координаты — градусы × 10⁷. Код выхода 0 — позиция друга пришла.
//!
//! `--login DOMAIN` — настоящий сервер (регистрация и вход, DOMAIN — его имя в
//! подписи входа) вместо dev-сервера. Без TLS: к развёрнутому серверу — через
//! SSH-туннель к его loopback-порту. Приглашение тогда передаётся через файл:
//! `--invite-file PATH` (invite пишет, accept ждёт и читает). Код приглашения
//! закрытого сервера — из `STAYA_INVITE_CODE` (не аргументом: не светится в `ps`).

use std::process::ExitCode;
use std::time::{Duration, Instant};

use dev_peer::{Error, Exchange, Peer};

fn pair(s: &str) -> Result<(i32, i32), Error> {
    let (a, b) = s.split_once(',').ok_or("expected LAT,LON")?;
    Ok((a.trim().parse()?, b.trim().parse()?))
}

fn run() -> Result<(), Error> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let arg = |name: &str| -> Option<String> {
        args.iter()
            .position(|a| a == name)
            .and_then(|i| args.get(i + 1).cloned())
    };
    let server = arg("--server").unwrap_or_else(|| "http://127.0.0.1:8787".into());
    let role = arg("--role").ok_or("--role invite|accept")?;
    let secs = |name: &str, default: u64| -> Result<Duration, Error> {
        Ok(Duration::from_secs(
            arg(name).map_or(Ok(default), |s| s.parse())?,
        ))
    };
    let exchange = Exchange {
        mine: pair(&arg("--mine").ok_or("--mine LAT,LON")?)?,
        expect: pair(&arg("--expect").ok_or("--expect LAT,LON")?)?,
        timeout: secs("--timeout", 600)?,
        linger: secs("--linger", 30)?,
    };

    let mut peer = Peer::new(&server)?;
    let login = arg("--login");
    let invite_file = arg("--invite-file");
    if let Some(domain) = &login {
        peer.login(
            domain,
            std::env::var("STAYA_INVITE_CODE")
                .ok()
                .filter(|c| !c.is_empty()),
        )?;
    }
    peer.publish_keys()?;
    match role.as_str() {
        "invite" => {
            // В dev адрес сервера в приглашении не используется: приложения идут на
            // заданный при запуске адрес (эмулятор видит хост как 10.0.2.2).
            let host = login.clone().unwrap_or_else(|| {
                server
                    .trim_start_matches("http://")
                    .trim_end_matches('/')
                    .to_owned()
            });
            let uri = peer.create_invite(&host)?;
            match &invite_file {
                Some(path) => std::fs::write(path, &uri)?,
                None => peer.post_invite(&uri)?,
            }
            eprintln!("PEER INVITE POSTED");
        }
        "accept" => {
            let deadline = Instant::now() + exchange.timeout;
            let uri = match &invite_file {
                Some(path) => loop {
                    if let Ok(uri) = std::fs::read_to_string(path)
                        && !uri.is_empty()
                    {
                        break uri;
                    }
                    if Instant::now() > deadline {
                        return Err("no invite file".into());
                    }
                    std::thread::sleep(Duration::from_millis(300));
                },
                None => peer.fetch_invite(deadline)?,
            };
            peer.accept(&uri)?;
            eprintln!("PEER ACCEPTED INVITE");
        }
        other => return Err(format!("unknown role {other}").into()),
    }
    exchange.run(&peer)
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("dev-peer: {e}");
            ExitCode::FAILURE
        }
    }
}
