use crate::env::CONFIG;
use crate::partitionner::SceneUuid;
use crate::serialization::serialize;
use crate::simulation::SimulationBounds;
use log::warn;
use serde::Serialize;
use std::collections::HashSet;
use std::str::FromStr;
use uuid::Uuid;
use zenoh::config::{ConnectConfig, EndPoint, PluginLoad, WhatAmI};
use zenoh::plugins::PluginsManager;
use zenoh::prelude::r#async::*;
use zenoh::publication::Publisher;
use zenoh::runtime::Runtime;

pub struct ZenohContext {
    pub session: Session,
}

impl ZenohContext {
    pub async fn new(
        mode: WhatAmI,
        endpoint: Option<String>,
        load_config_file: bool,
    ) -> anyhow::Result<Self> {
        let mut config = Config::default();

        if load_config_file {
            config = match Config::from_file("zenoh.json5") {
                Ok(c) => c,
                Err(e) => {
                    warn!("Failed to load zenoh config file: {e}");
                    Config::default()
                }
            };
        }

        let _ = config.set_mode(Some(mode));

        match mode {
            WhatAmI::Client => {
                config.connect = ConnectConfig::new(vec![EndPoint::from_str(
                    endpoint.as_deref().unwrap_or("tcp/localhost:7447"),
                )
                .unwrap()])
                .unwrap();
            }
            WhatAmI::Router => {
                if !CONFIG.zenoh_router.is_empty() {
                    config.connect = ConnectConfig::new(vec![EndPoint::from_str(
                        endpoint.as_ref().unwrap_or(&CONFIG.zenoh_router),
                    )
                    .unwrap()])
                    .unwrap();
                }
            }
            WhatAmI::Peer => {}
        }

        let session = zenoh::open(config.clone()).res().await.unwrap();
        load_zenoh_plugins(config, session.runtime()).await;
        Ok(Self { session })
    }

    pub async fn put(&self, queue: &str, elt: &impl Serialize) -> anyhow::Result<()> {
        let publisher = self
            .session
            .declare_publisher(queue)
            .congestion_control(CongestionControl::Block)
            .res()
            .await
            .unwrap();
        put(&publisher, elt).await
    }
}

pub async fn put(publisher: &Publisher<'_>, elt: &impl Serialize) -> anyhow::Result<()> {
    let data = serialize(elt)?;
    publisher.put(data).res().await.expect("F");
    Ok(())
}

pub fn runner_zenoh_commands_key(uuid: Uuid) -> String {
    format!("runner/{}", uuid.to_string())
}

pub fn runner_zenoh_ack_key(scene: SceneUuid, region: &SimulationBounds) -> String {
    format!("ack/{}/{}", scene.0, region.to_string())
}

pub fn zenoh_storage_key(key: &str) -> String {
    format!("steadyum/kv/{key}")
}

// Loads zenoh plugins.
// This is mostly copied from https://github.com/eclipse-zenoh/zenoh/blob/master/zenohd/src/main.rs
async fn load_zenoh_plugins(config: Config, runtime: &Runtime) {
    log::info!("Initial conf: {}", &config);

    let mut plugins = PluginsManager::dynamic(config.libloader());
    // Static plugins are to be added here, with `.add_static::<PluginType>()`
    let mut required_plugins = HashSet::new();
    for plugin_load in config.plugins().load_requests() {
        let PluginLoad {
            name,
            paths,
            required,
        } = plugin_load;
        log::info!(
            "Loading {req} plugin \"{name}\"",
            req = if required { "required" } else { "" }
        );
        if let Err(e) = match paths {
            None => plugins.load_plugin_by_name(name.clone()),
            Some(paths) => plugins.load_plugin_by_paths(name.clone(), &paths),
        } {
            if required {
                panic!("Plugin load failure: {}", e)
            } else {
                log::error!("Plugin load failure: {}", e)
            }
        }
        if required {
            required_plugins.insert(name);
        }
    }

    for (name, path, start_result) in plugins.start_all(runtime) {
        let required = required_plugins.contains(name);
        log::info!(
            "Starting {req} plugin \"{name}\"",
            req = if required { "required" } else { "" }
        );
        match start_result {
            Ok(Some(_)) => log::info!("Successfully started plugin {} from {:?}", name, path),
            Ok(None) => log::warn!("Plugin {} from {:?} wasn't loaded, as an other plugin by the same name is already running", name, path),
            Err(e) => {
                let report = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| e.to_string())) {
                    Ok(s) => s,
                    Err(_) => panic!("Formatting the error from plugin {} ({:?}) failed, this is likely due to ABI unstability.\r\nMake sure your plugin was built with the same version of cargo as zenohd", name, path),
                };
                if required {
                    panic!("Plugin \"{name}\" failed to start: {}", if report.is_empty() {"no details provided"} else {report.as_str()});
                }else {
                    log::error!("Required plugin \"{name}\" failed to start: {}", if report.is_empty() {"no details provided"} else {report.as_str()});
                }
            }
        }
    }
    log::info!("Finished loading plugins");

    {
        let mut config_guard = runtime.config.lock();
        for (name, (_, plugin)) in plugins.running_plugins() {
            let hook = plugin.config_checker();
            config_guard.add_plugin_validator(name, hook)
        }
    }
}
