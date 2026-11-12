//! 契约的线格式（位置点、健康样本、保存期限）。权威定义在核心仓库 `wonton-protocol` 的 `personal_data.rs`，
//! 这里只放这条能力用到的形状：提供方要多认的字段忽略，不认识的取值落到 `Other`。

use serde::{Deserialize, Serialize};

pub const LOCATION: &str = "location";

/// 契约的路径，和核心仓库 `wonton-protocol` 的常量一一对应。
pub const LOCATION_PATH: &str = "/location";
pub const HEALTH_PATH: &str = "/health";
pub const QUERY_PATH: &str = "/query";
pub const DATA_PATH: &str = "/data";
pub const EXPORT_PATH: &str = "/export";
pub const CLEANUP_PATH: &str = "/cleanup";

/// 核心定的类型清单里属于健康的那些；不认识的类型按健康类型收，只是还没有健康的名字。
pub const HEALTH_TYPES: [&str; 7] = [
    "steps",
    "walking_running_distance",
    "active_energy",
    "heart_rate",
    "resting_heart_rate",
    "sleep",
    "body_weight",
];

#[derive(Deserialize)]
pub struct LocationPoint {
    #[serde(rename = "_type")]
    pub kind: LocationKind,
    pub lat: f64,
    pub lon: f64,
    pub acc: f64,
    /// 测量时间，Unix 秒。
    pub tst: u64,
    pub tid: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LocationKind {
    Location,
    #[serde(other)]
    Other,
}

#[derive(Deserialize)]
pub struct HealthSample {
    pub type_name: String,
    /// 样本开始或点样本的时刻，Unix 毫秒。
    pub start_ms: u64,
    /// 段的结束；点样本为空。
    pub end_ms: Option<u64>,
    pub value: HealthValue,
    /// 手机侧的去重键，同一段重传不变。
    pub source_id: String,
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum HealthValue {
    Quantity {
        value: f64,
        unit: String,
    },
    Sleep {
        stage: SleepStage,
    },
    #[serde(other)]
    Other,
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SleepStage {
    InBed,
    Asleep,
    Core,
    Deep,
    Rem,
    Awake,
    #[serde(other)]
    Other,
}

#[derive(Deserialize)]
pub struct HealthSamples {
    pub samples: Vec<HealthSample>,
    pub synced_through_ms: Option<u64>,
}

#[derive(Deserialize, Serialize, Clone)]
#[serde(tag = "policy", rename_all = "snake_case")]
pub enum RetentionPolicy {
    /// 不保存：收下就删。
    Never,
    Days {
        days: u32,
    },
    /// 比本提供方新的核心给的新期限；按最长的处理，不提前删。
    #[serde(other)]
    Other,
}

#[derive(Deserialize)]
pub struct CleanupRequest {
    pub policies: Vec<RetentionSetting>,
}

#[derive(Deserialize)]
pub struct RetentionSetting {
    pub type_name: String,
    pub retention: RetentionPolicy,
}

impl HealthValue {
    /// 收下之前看一遍写法：数值要有限，单位不能空。
    pub fn valid(&self) -> bool {
        match self {
            Self::Quantity { value, unit } => value.is_finite() && !unit.is_empty(),
            Self::Sleep { stage } => matches!(
                stage,
                SleepStage::InBed
                    | SleepStage::Asleep
                    | SleepStage::Core
                    | SleepStage::Deep
                    | SleepStage::Rem
                    | SleepStage::Awake
                    | SleepStage::Other
            ),
            Self::Other => true,
        }
    }
}
