use crate::AppState;
use futures::select;
use std::collections::HashMap;
use std::sync::Arc;
use zenoh::prelude::r#async::AsyncResolve;
use zenoh::prelude::{keyexpr, SampleKind};
use zenoh::sample::Sample;

pub fn start_storage_thread(app: AppState) {
    let _ = std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(storage_loop(&app))
    });
}

async fn storage_loop(app: &AppState) {
    let key_expr = "steadyum/kv/**".to_string();

    println!("Declaring Subscriber on '{key_expr}'...");

    let subscriber = app
        .data
        .zenoh
        .session
        .declare_subscriber(&key_expr)
        .res()
        .await
        .unwrap();

    let mut stored: HashMap<String, Sample> = HashMap::new();

    println!("Declaring Queryable on '{key_expr}'...");
    let queryable = app
        .data
        .zenoh
        .session
        .declare_queryable(&key_expr)
        .complete(true)
        .res()
        .await
        .unwrap();

    loop {
        select!(
            sample = subscriber.recv_async() => {
                let sample = sample.unwrap();
                // println!(">> [Subscriber] Received {} ('{}': '{}')",
                //     sample.kind, sample.key_expr.as_str(), sample.value);
                if sample.kind == SampleKind::Delete {
                    stored.remove(&sample.key_expr.to_string());
                } else {
                    stored.insert(sample.key_expr.to_string(), sample);
                }
            },

            query = queryable.recv_async() => {
                let query = query.unwrap();
                // println!(">> [Queryable ] Received Query '{}'", query.selector());
                for (stored_name, sample) in stored.iter() {
                    if query.selector().key_expr.intersects(unsafe {keyexpr::from_str_unchecked(stored_name)}) {
                        query.reply(Ok(sample.clone())).res().await.unwrap();
                    }
                }
            }
        );
    }
}
