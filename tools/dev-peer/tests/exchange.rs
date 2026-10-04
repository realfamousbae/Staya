//! Два ядра обмениваются позициями через настоящий dev-сервер по HTTP (задача 2.10).

use std::time::Duration;

use dev_peer::{Exchange, Peer};
use staya_server::dev::{DevState, app};

fn start_server() -> String {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            tx.send(listener.local_addr().unwrap()).unwrap();
            axum::serve(listener, app(DevState::default()))
                .await
                .unwrap();
        });
    });
    format!("http://{}", rx.recv().unwrap())
}

const ALICE: (i32, i32) = (557_558_000, 376_173_000);
const BOB: (i32, i32) = (599_386_000, 303_141_000);

#[test]
fn two_cores_exchange_locations_over_http() {
    let base = start_server();
    let alice = Peer::new(&base).unwrap();
    let bob = Peer::new(&base).unwrap();
    alice.publish_keys().unwrap();
    bob.publish_keys().unwrap();

    let host = base.trim_start_matches("http://").to_owned();
    alice
        .post_invite(&alice.create_invite(&host).unwrap())
        .unwrap();
    let uri = bob
        .fetch_invite(std::time::Instant::now() + Duration::from_secs(5))
        .unwrap();
    bob.accept(&uri).unwrap();

    let run = |peer: Peer, mine, expect| {
        std::thread::spawn(move || {
            Exchange {
                mine,
                expect,
                timeout: Duration::from_secs(60),
                linger: Duration::from_secs(3),
            }
            .run(&peer)
            .map(|()| peer)
            .map_err(|e| e.to_string())
        })
    };
    let a = run(alice, ALICE, BOB);
    let b = run(bob, BOB, ALICE);
    let alice = a.join().unwrap().expect("alice got bob's location");
    let bob = b.join().unwrap().expect("bob got alice's location");
    assert!(alice.has_friends().unwrap() && bob.has_friends().unwrap());
}
