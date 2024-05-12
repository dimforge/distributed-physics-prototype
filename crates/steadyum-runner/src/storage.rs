use crate::{runner, AppState};
use log::{error, info};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use steadyum_api_types::objects::{ClientBodyObjectSet, WatchedObjects};
use steadyum_api_types::serialization::serialize;
use steadyum_api_types::simulation::SimulationBounds;
use zenoh::prelude::r#async::AsyncResolve;
use zenoh::sample::Sample;

pub fn start_storage_thread_for_watched_objects(app: Arc<AppState>) {
    let _ = std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(listen_storage_queries_for_watched_objects(&app))
    });
}

pub async fn listen_storage_queries_for_watched_objects(app: &AppState) {
    // NOTE: we only need a queryable to expose access to the watch sets.
    //       Inserting data into the watch set is done entirely locally.
    let key_expr = format!("steadyum/watch/{:?}", app.uuid);

    info!("Starting watch storage: {}", key_expr);

    let queryable = app
        .zenoh
        .session
        .declare_queryable(&key_expr)
        .complete(true)
        .res()
        .await
        .unwrap();

    while !app.exit.load(Ordering::SeqCst) {
        let query = queryable.recv_async().await;
        let Ok(query) = query else { break };
        let selector = query.selector();
        // println!(">> [Queryable ] Received Query '{}'", query.selector());
        let params = selector.parameters();
        let Some(region) = SimulationBounds::from_str(params) else {
            continue;
        };
        // println!(">>>>> [Queryable ] Region {:?} was queried.", region);
        let data = app
            .watch_sets
            .get(&region)
            .map(|obj| {
                // info!("Answering {} watched objects.", obj.value().objects.len());
                serialize(obj.value()).unwrap()
            })
            .unwrap_or_else(|| serialize(&WatchedObjects::default()).unwrap());

        let sample = Sample::new(query.key_expr().clone(), data);

        query.reply(Ok(sample)).res().await;
    }

    info!("Exiting storage loop.")
}

pub fn start_storage_thread_for_client_objects(app: Arc<AppState>) {
    let _ = std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(crate::storage::listen_storage_queries_for_client_objects(
            &app,
        ))
    });
}

pub async fn listen_storage_queries_for_client_objects(app: &AppState) {
    // NOTE: we only need a queryable to expose access to the body sets.
    //       Inserting data into the body set is done entirely locally.
    let key_expr = format!("steadyum/client_bodies/{:?}", app.scene.0);

    info!("Starting bodies storage: {}", key_expr);

    let queryable = app
        .zenoh
        .session
        .declare_queryable(&key_expr)
        .complete(true)
        .res()
        .await
        .unwrap();

    while !app.exit.load(Ordering::SeqCst) {
        let query = queryable.recv_async().await;
        let Ok(query) = query else { break };
        let selector = query.selector();
        // println!(">> [Queryable ] Received Query '{}'", query.selector());

        let mut params = selector.parameters().split('&');
        let Some(region_str) = params.next() else {
            continue;
        };
        let Some(step_id_str) = params.next() else {
            continue;
        };

        let Some(region) = SimulationBounds::from_str(region_str) else {
            continue;
        };

        use std::str::FromStr;
        let Ok(step_id) = u64::from_str(step_id_str) else {
            continue;
        };

        // println!(">>>>> [Queryable ] Region {:?} was queried.", region);
        let data = app
            .client_object_sets
            .get(&region)
            .map(|obj| serialize(&filter_object_set(step_id, obj.value())).unwrap())
            .unwrap_or_else(|| serialize(&ClientBodyObjectSet::default()).unwrap());

        let sample = Sample::new(query.key_expr().clone(), data);

        if let Err(e) = query.reply(Ok(sample)).res().await {
            error!("Error replying to client objects query: {e}");
        }
    }

    info!("Exiting storage loop.")
}

fn filter_object_set(step_id: u64, object_set: &ClientBodyObjectSet) -> ClientBodyObjectSet {
    let mut result = object_set.clone();
    let mut max_sleep_frame = 0;

    result.objects.retain(|obj| {
        if let Some(sleep_frame) = obj.sleep_start_frame {
            // Don’t keep object that we know were already asleep during the last frame.
            max_sleep_frame = max_sleep_frame.max(sleep_frame);
            sleep_frame >= step_id
        } else {
            true
        }
    });

    // println!(
    //     ">>>>>>>>>>>>> RESULT: {} vs  {}, {} (max sleep frame: {})",
    //     step_id,
    //     result.timestamp,
    //     result.objects.len(),
    //     max_sleep_frame,
    // );
    result
}
