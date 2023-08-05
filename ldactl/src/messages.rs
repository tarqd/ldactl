use crate::credential::{ClientSideId as EnvironmentId, MobileKey, ServerSideKey};
use serde::{de::Error, Deserialize, Deserializer, Serialize};

use std::{
    collections::HashMap,
    fmt::{self, Display, Formatter},
};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ProjectKey(String);

impl Display for ProjectKey {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}
impl AsRef<str> for ProjectKey {
    fn as_ref(&self) -> &str {
        self.0.as_ref()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct EnvironmentKey(String);

impl AsRef<str> for EnvironmentKey {
    fn as_ref(&self) -> &str {
        self.0.as_ref()
    }
}
impl Display for EnvironmentKey {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

type Version = u64;
type UnixTimestamp = u64;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentConfig {
    #[serde(rename = "envId")]
    pub env_id: EnvironmentId,
    pub env_key: EnvironmentKey,
    pub env_name: String,
    pub mob_key: MobileKey,
    pub proj_key: ProjectKey,
    pub proj_name: String,
    pub sdk_key: Expirable<ServerSideKey>,
    pub default_ttl: u64,
    pub secure_mode: bool,
    pub version: Version,
}

fn deserialize_env_id_from_path<'de, D>(deserializer: D) -> Result<EnvironmentId, D::Error>
where
    D: Deserializer<'de>,
{
    const PATH_PREFIX: &'static str = "/environments/";
    let buf = String::deserialize(deserializer)?;
    let s = buf
        .strip_prefix(PATH_PREFIX)
        .ok_or_else(|| D::Error::custom("invalid path"))?;
    EnvironmentId::try_from(s).map_err(D::Error::custom)
}
fn serialize_env_id_path<S>(env_id: &EnvironmentId, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    serializer.collect_str(&format_args!("/environments/{}", env_id))
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PatchEvent {
    #[serde(
        deserialize_with = "deserialize_env_id_from_path",
        serialize_with = "serialize_env_id_path"
    )]
    #[serde(rename = "path")]
    pub env_id: EnvironmentId,
    #[serde(rename = "data")]
    pub environment: EnvironmentConfig,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PutData {
    pub environments: HashMap<EnvironmentId, EnvironmentConfig>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PutEvent {
    pub path: String,
    pub data: PutData,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteEvent {
    #[serde(
        deserialize_with = "deserialize_env_id_from_path",
        serialize_with = "serialize_env_id_path",
        rename = "path"
    )]
    pub env_id: EnvironmentId,
    pub version: Version,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Message {
    Put(PutEvent),
    Patch(PatchEvent),
    Delete(DeleteEvent),
    Reconnect,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Expirable<T> {
    #[serde(rename = "value")]
    current: T,
    expiring: Option<Expiring<T>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Expiring<T> {
    value: T,
    expires_at: UnixTimestamp,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::credential::ClientSideId;
    #[test]
    fn derialize_put() {
        let _ev = "{\"path\":\"/\",\"data\":{\"environments\":{\"62ea8c4afac9b011945f6791\":{\"envId\":\"62ea8c4afac9b011945f6791\",\"envKey\":\"test\",\"envName\":\"Test\",\"mobKey\":\"mob-b5734766-5a3d-4b41-b63f-2669a4fb6497\",\"projName\":\"Default\",\"projKey\":\"default\",\"sdkKey\":{\"value\":\"sdk-3d560391-904c-4afd-8075-faad7652ed1d\"},\"defaultTtl\":0,\"secureMode\":false,\"version\":6},\"62ea8c4afac9b011945f6792\":{\"envId\":\"62ea8c4afac9b011945f6792\",\"envKey\":\"production\",\"envName\":\"Production\",\"mobKey\":\"mob-6a161a22-6395-4c29-a9cd-88d4b5bf74d6\",\"projName\":\"Default\",\"projKey\":\"default\",\"sdkKey\":{\"value\":\"sdk-011511cd-335b-47af-9e01-05a0daf1d71e\"},\"defaultTtl\":0,\"secureMode\":false,\"version\":14},\"64a447c454eaac132a068d75\":{\"envId\":\"64a447c454eaac132a068d75\",\"envKey\":\"production\",\"envName\":\"Production\",\"mobKey\":\"mob-aa46ddd0-5d78-44d5-9337-c6bbd9965feb\",\"projName\":\"Example project\",\"projKey\":\"example-project\",\"sdkKey\":{\"value\":\"sdk-6c596994-34d0-4137-84c6-bef64a1732d0\"},\"defaultTtl\":0,\"secureMode\":false,\"version\":20},\"64a447c454eaac132a068d76\":{\"envId\":\"64a447c454eaac132a068d76\",\"envKey\":\"test\",\"envName\":\"Test\",\"mobKey\":\"mob-ca268c40-7c6b-4b36-a30c-0f9e93b68751\",\"projName\":\"Example project\",\"projKey\":\"example-project\",\"sdkKey\":{\"value\":\"sdk-35cbaa92-d78a-4c5e-aecf-52de2933e289\"},\"defaultTtl\":0,\"secureMode\":false,\"version\":20}}}}";
        let s = r#"
        {
            "path": "/",
            "data": {
              "environments": {
                "62ea8c4afac9b011945f6791": {
                    "envId":"62ea8c4afac9b011945f6791",
                    "envKey":"test",
                    "envName":"Test",
                    "mobKey":
                    "mob-b5734766-5a3d-4b41-b63f-2669a4fb6497",
                    "projName":"Default",
                    "projKey":"default",
                    "sdkKey":{"value":"sdk-3d560391-904c-4afd-8075-faad7652ed1d"},
                    "defaultTtl":0,
                    "secureMode":false,
                    "version":6
                }
            }
        }}
        "#;
        let ret = serde_json::from_str::<PutEvent>(s);
        assert!(ret.is_ok(), "{:?}", ret);
    }
    #[test]
    fn test_deserialize_env_id_from_path() {
        use super::deserialize_env_id_from_path;
        use crate::credential::ClientSideId as EnvironmentId;
        let mut d =
            serde_json::Deserializer::from_str("\"/environments/62ea8c4afac9b011945f6792\"");
        assert_eq!(
            deserialize_env_id_from_path(&mut d).unwrap(),
            EnvironmentId::try_from("62ea8c4afac9b011945f6792").unwrap()
        );
        let error_cases = &[
            "\"/environments/\"",
            "\"/environments\"",
            "\"/environments/FOOO\"",
            "\"/environments/62ea8c4afac9b011945f6792333\"",
        ];

        for case in error_cases {
            let mut d = serde_json::Deserializer::from_str(case);
            assert!(deserialize_env_id_from_path(&mut d).is_err());
        }
    }
    #[test]
    fn test_serialize_env_id_path() {
        let env = "62ea8c4afac9b011945f6792";
        let path = format!("\"/environments/{}\"", env);

        let mut w = std::io::BufWriter::new(Vec::new());
        let mut se = serde_json::Serializer::new(&mut w);
        let result = super::serialize_env_id_path(&ClientSideId::try_from(env).unwrap(), &mut se);
        assert!(result.is_ok());
        assert_eq!(String::from_utf8(w.into_inner().unwrap()).unwrap(), path);
    }
}
