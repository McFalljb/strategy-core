use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EventDelivery {
    #[default]
    Wake,
    Decision,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RuntimeCapabilities {
    #[serde(default)]
    pub supports_http: bool,
    #[serde(default = "default_supports_data_queries")]
    pub supports_data_queries: bool,
    #[serde(default)]
    pub supports_one_shot_timers: bool,
    #[serde(default)]
    pub supports_recurring_timers: bool,
    #[serde(default)]
    pub supports_native_kernels: bool,
    #[serde(default)]
    pub queue_is_durable: bool,
    #[serde(default)]
    pub replay_controls_event_progression: bool,
    #[serde(default)]
    pub event_delivery: EventDelivery,
}

impl Default for RuntimeCapabilities {
    fn default() -> Self {
        Self {
            supports_http: false,
            supports_data_queries: true,
            supports_one_shot_timers: false,
            supports_recurring_timers: false,
            supports_native_kernels: false,
            queue_is_durable: false,
            replay_controls_event_progression: false,
            event_delivery: EventDelivery::Wake,
        }
    }
}

const fn default_supports_data_queries() -> bool {
    true
}
