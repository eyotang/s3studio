use serde::{Deserialize, Serialize};
use std::{collections::HashMap, path::PathBuf};
use tokio::sync::Mutex;
use uuid::Uuid;

const SERVICE: &str = "com.s3studio.desktop";

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub region: String,
    #[serde(default)]
    pub endpoint: String,
    pub path_style: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountInput {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub region: String,
    #[serde(default)]
    pub endpoint: String,
    pub path_style: bool,
    pub access_key: String,
    pub secret_key: String,
    pub session_token: String,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
#[serde(default)]
pub struct S3Defaults {
    pub region: String,
    pub endpoint: String,
}

impl S3Defaults {
    pub fn endpoint_for<'a>(&'a self, account: &'a Account) -> &'a str {
        if account.endpoint.is_empty() {
            &self.endpoint
        } else {
            &account.endpoint
        }
    }

    pub fn region_for<'a>(&'a self, account: &'a Account) -> &'a str {
        if !account.region.is_empty() {
            &account.region
        } else if !self.region.is_empty() {
            &self.region
        } else {
            "us-east-1"
        }
    }
}

// Never serialize this type into frontend responses or account configuration files.
#[derive(Clone, Serialize, Deserialize)]
pub struct Secret {
    pub access_key: String,
    pub secret_key: String,
    pub session_token: String,
}

pub struct AccountStore {
    pub accounts: Mutex<Vec<Account>>,
    pub bucket_regions: Mutex<HashMap<(String, String), String>>,
    path: PathBuf,
    defaults: Mutex<S3Defaults>,
    defaults_path: PathBuf,
}

impl AccountStore {
    pub fn open(path: PathBuf) -> Result<Self, Box<dyn std::error::Error>> {
        let accounts = match std::fs::read(&path) {
            Ok(data) => serde_json::from_slice(&data)?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(e) => return Err(e.into()),
        };
        let defaults_path = path.with_extension("s3-defaults.json");
        let defaults = match std::fs::read(&defaults_path) {
            Ok(data) => serde_json::from_slice(&data)?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => S3Defaults::default(),
            Err(e) => return Err(e.into()),
        };
        Ok(Self {
            accounts: Mutex::new(accounts),
            bucket_regions: Mutex::new(HashMap::new()),
            path,
            defaults: Mutex::new(defaults),
            defaults_path,
        })
    }

    async fn persist(&self, accounts: &[Account]) -> Result<(), String> {
        let data = serde_json::to_vec_pretty(accounts).map_err(|e| e.to_string())?;
        persist_config(&self.path, data).await
    }
}

async fn persist_config(path: &std::path::Path, data: Vec<u8>) -> Result<(), String> {
    let temporary = path.with_extension("json.tmp");
    tokio::fs::write(&temporary, data)
        .await
        .map_err(|e| crate::lf!("无法保存账户配置：{e}", e = e))?;
    tokio::fs::rename(temporary, path)
        .await
        .map_err(|e| crate::lf!("无法替换账户配置：{e}", e = e))
}

pub async fn get_s3_defaults(store: &AccountStore) -> S3Defaults {
    store.defaults.lock().await.clone()
}

pub async fn set_s3_defaults(
    store: &AccountStore,
    input: S3Defaults,
) -> Result<S3Defaults, String> {
    let input = S3Defaults {
        region: input.region.trim().into(),
        endpoint: input.endpoint.trim().into(),
    };
    validate_connection(&input.region, &input.endpoint)?;
    let input = S3Defaults {
        endpoint: input.endpoint.trim_end_matches('/').into(),
        ..input
    };
    let mut defaults = store.defaults.lock().await;
    let data = serde_json::to_vec_pretty(&input).map_err(|e| e.to_string())?;
    persist_config(&store.defaults_path, data).await?;
    *defaults = input.clone();
    Ok(input)
}

fn keyring_error(e: keyring::Error) -> String {
    crate::lf!(
        "系统密钥库操作失败：{e}。请确认密钥库已解锁；Ubuntu 需启用 Secret Service。",
        e = e
    )
}

async fn read_optional_secret(id: String) -> Result<Option<Secret>, String> {
    tokio::task::spawn_blocking(move || {
        let entry = keyring::Entry::new(SERVICE, &id).map_err(keyring_error)?;
        let value = match entry.get_password() {
            Ok(value) => value,
            Err(keyring::Error::NoEntry) => return Ok(None),
            Err(e) => return Err(keyring_error(e)),
        };
        serde_json::from_str(&value)
            .map(Some)
            .map_err(|_| crate::i18n::t("凭据数据损坏，请重新设置账户凭据。").to_owned())
    })
    .await
    .map_err(|e| e.to_string())?
}

pub async fn read_secret(id: String) -> Result<Secret, String> {
    read_optional_secret(id)
        .await?
        .ok_or(crate::i18n::t("凭据不存在，请在账户设置中重新填写访问密钥。").into())
}

async fn write_secret(id: String, secret: Secret) -> Result<(), String> {
    tokio::task::spawn_blocking(move || {
        let entry = keyring::Entry::new(SERVICE, &id).map_err(keyring_error)?;
        entry
            .set_password(&serde_json::to_string(&secret).map_err(|e| e.to_string())?)
            .map_err(keyring_error)
    })
    .await
    .map_err(|e| e.to_string())?
}

async fn delete_secret(id: String) -> Result<(), String> {
    tokio::task::spawn_blocking(move || {
        let entry = keyring::Entry::new(SERVICE, &id).map_err(keyring_error)?;
        match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(keyring_error(e)),
        }
    })
    .await
    .map_err(|e| e.to_string())?
}

fn validate_connection(region: &str, endpoint: &str) -> Result<(), String> {
    if !region
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-')
    {
        return Err(crate::i18n::t("请输入有效的 Region，例如 us-east-1。").into());
    }
    if !endpoint.is_empty() {
        let url = url::Url::parse(endpoint)
            .map_err(|_| crate::i18n::t("Endpoint 必须是完整的 HTTP / HTTPS URL。"))?;
        if !matches!(url.scheme(), "https" | "http")
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err(crate::i18n::t(
                "Endpoint 只能使用 HTTP / HTTPS，且不能包含凭据、查询参数或片段。",
            )
            .into());
        }
    }
    Ok(())
}

fn validate(input: &AccountInput) -> Result<(), String> {
    if input.name.trim().is_empty() || input.name.chars().count() > 80 {
        return Err(crate::i18n::t("账户名称需为 1–80 个字符。").into());
    }
    validate_connection(&input.region, &input.endpoint)?;
    if input.access_key.is_empty() != input.secret_key.is_empty() {
        return Err(crate::i18n::t("更新凭据时必须同时填写 Access Key 和 Secret Key。").into());
    }
    if input.access_key.is_empty() && !input.session_token.is_empty() {
        return Err(
            crate::i18n::t("更新 Session Token 时请同时填写 Access Key 和 Secret Key。").into(),
        );
    }
    Ok(())
}

pub async fn list_accounts(store: &AccountStore) -> Result<Vec<Account>, String> {
    Ok(store.accounts.lock().await.clone())
}

pub async fn save_account(store: &AccountStore, input: AccountInput) -> Result<Account, String> {
    validate(&input)?;
    let mut accounts = store.accounts.lock().await;
    let existing = if input.id.is_empty() {
        None
    } else {
        Some(
            accounts
                .iter()
                .position(|a| a.id == input.id)
                .ok_or(crate::i18n::t("账户不存在，请重新加载。"))?,
        )
    };
    if existing.is_none() && input.access_key.is_empty() {
        return Err(crate::i18n::t("新账户必须提供访问凭据。").into());
    }
    let id = if existing.is_some() {
        input.id
    } else {
        Uuid::new_v4().to_string()
    };
    let account = Account {
        id: id.clone(),
        name: input.name.trim().into(),
        region: input.region,
        endpoint: input.endpoint.trim_end_matches('/').into(),
        path_style: input.path_style,
    };
    let previous_secret = if existing.is_some() && !input.access_key.is_empty() {
        read_optional_secret(id.clone()).await?
    } else {
        None
    };
    let updated_secret = !input.access_key.is_empty();
    if updated_secret {
        write_secret(
            id.clone(),
            Secret {
                access_key: input.access_key,
                secret_key: input.secret_key,
                session_token: input.session_token,
            },
        )
        .await?;
    }
    let mut updated = accounts.clone();
    if let Some(index) = existing {
        updated[index] = account.clone();
    } else {
        updated.push(account.clone());
    }
    if let Err(error) = store.persist(&updated).await {
        if updated_secret {
            let rollback = if let Some(secret) = previous_secret {
                write_secret(id.clone(), secret).await
            } else {
                delete_secret(id.clone()).await
            };
            if let Err(rollback_error) = rollback {
                return Err(crate::lf!(
                    "{error}；凭据回滚失败：{rollback_error}",
                    error = error,
                    rollback_error = rollback_error
                ));
            }
        }
        return Err(error);
    }
    *accounts = updated;
    store
        .bucket_regions
        .lock()
        .await
        .retain(|(account_id, _), _| account_id != &id);
    Ok(account)
}

pub async fn remove_account(
    store: &AccountStore,
    engine: &crate::transfers::Engine,
    account_id: String,
) -> Result<(), String> {
    let mut accounts = store.accounts.lock().await;
    if engine.has_unfinished_account(&account_id).await {
        return Err(
            crate::i18n::t("此账户仍有未结束的传输，请先完成或取消任务再移除账户。").into(),
        );
    }
    let index = accounts
        .iter()
        .position(|a| a.id == account_id)
        .ok_or(crate::i18n::t("账户不存在。"))?;
    let previous = read_optional_secret(account_id.clone()).await?;
    delete_secret(account_id.clone()).await?;
    let mut updated = accounts.clone();
    updated.remove(index);
    if let Err(e) = store.persist(&updated).await {
        if let Some(previous) = previous {
            write_secret(account_id.clone(), previous)
                .await
                .map_err(|rollback| {
                    crate::lf!("{e}；凭据回滚失败：{rollback}", rollback = rollback, e = e)
                })?;
        }
        return Err(e);
    }
    *accounts = updated;
    store
        .bucket_regions
        .lock()
        .await
        .retain(|(id, _), _| id != &account_id);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn input() -> AccountInput {
        AccountInput {
            id: "".into(),
            name: "Test".into(),
            region: "us-east-1".into(),
            endpoint: "".into(),
            path_style: false,
            access_key: "test".into(),
            secret_key: "secret".into(),
            session_token: "".into(),
        }
    }
    #[tokio::test]
    async fn s3_defaults_persist_without_overwriting_account_fields() {
        let dir = std::env::temp_dir().join(format!("s3-defaults-{}", Uuid::new_v4()));
        std::fs::create_dir(&dir).unwrap();
        let path = dir.join("accounts.json");
        let accounts = r#"[{"id":"legacy","name":"Legacy","region":"eu-west-1","pathStyle":false},
            {"id":"inherited","name":"Inherited","endpoint":"https://custom.example","pathStyle":true}]"#;
        std::fs::write(&path, accounts).unwrap();
        let store = AccountStore::open(path.clone()).unwrap();
        let defaults = set_s3_defaults(
            &store,
            S3Defaults {
                endpoint: " https://global.example/ ".into(),
                region: " ap-southeast-1 ".into(),
            },
        )
        .await
        .unwrap();
        let invalid = S3Defaults {
            endpoint: "file:///invalid".into(),
            ..defaults.clone()
        };
        assert!(set_s3_defaults(&store, invalid).await.is_err());
        let reopened = AccountStore::open(path.clone()).unwrap();
        assert_eq!(get_s3_defaults(&reopened).await, defaults);
        assert_eq!(std::fs::read_to_string(path).unwrap(), accounts);

        let mut accounts = list_accounts(&reopened).await.unwrap().into_iter();
        let legacy = accounts.next().unwrap();
        assert_eq!(defaults.region_for(&legacy), "eu-west-1");
        assert_eq!(defaults.endpoint_for(&legacy), "https://global.example");

        let inherited = accounts.next().unwrap();
        assert_eq!(S3Defaults::default().region_for(&inherited), "us-east-1");
        assert_eq!(defaults.region_for(&inherited), "ap-southeast-1");
        assert_eq!(defaults.endpoint_for(&inherited), "https://custom.example");

        set_s3_defaults(&reopened, S3Defaults::default())
            .await
            .unwrap();
        assert_eq!(get_s3_defaults(&reopened).await, S3Defaults::default());
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn rejects_invalid_endpoints_and_incomplete_credentials() {
        let mut i = input();
        i.endpoint = "file:///etc/passwd".into();
        assert!(validate(&i).is_err());
        i.endpoint = "https://user:password@example.com".into();
        assert!(validate(&i).is_err());
        i.endpoint = "https://example.com?token=secret".into();
        assert!(validate(&i).is_err());
        i.endpoint = "http://localhost:9000".into();
        assert!(validate(&i).is_ok());
        i.secret_key.clear();
        assert!(validate(&i).is_err());
    }
    #[test]
    fn metadata_never_contains_credentials_and_account_count_is_unlimited() {
        let items: Vec<_> = (0..150)
            .map(|n| Account {
                id: n.to_string(),
                name: format!("Account {n}"),
                region: "us-east-1".into(),
                endpoint: "".into(),
                path_style: false,
            })
            .collect();
        let json = serde_json::to_string(&items).unwrap();
        assert!(!json.contains("secret"));
        assert!(!json.contains("accessKey"));
        assert_eq!(
            serde_json::from_str::<Vec<Account>>(&json).unwrap().len(),
            150
        );
    }
}
