use chrono::{DateTime, Local};
use s3_studio_core::{s3::S3Object, transfers::Transfer};
use std::cmp::Ordering;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sort {
    Name,
    Size,
    Modified,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Dialog {
    Account(Option<String>),
    Folder,
    Bucket,
    Trash,
    Versions,
    LocatePath,
    Copy,
    Delete,
    Upload,
    RemoveAccount(String),
    Help,
    About,
    TransferSettings,
    S3Defaults,
    CancelTransfer(String),
    Update,
}
impl Dialog {
    pub fn title(&self) -> &'static str {
        match self {
            Self::Account(Some(_)) => crate::i18n::t("账户设置"),
            Self::Account(None) => crate::i18n::t("连接一个新账户"),
            Self::Folder => crate::i18n::t("新建文件夹"),
            Self::Bucket => crate::i18n::t("创建存储桶"),
            Self::Trash => crate::i18n::t("回收站"),
            Self::Versions => crate::i18n::t("版本历史"),
            Self::LocatePath => crate::i18n::t("定位路径"),
            Self::Copy => crate::i18n::t("复制对象"),
            Self::Delete => crate::i18n::t("删除所选项目？"),
            Self::Upload => crate::i18n::t("上传文件"),
            Self::RemoveAccount(_) => crate::i18n::t("删除工作空间配置？"),
            Self::Help => crate::i18n::t("为云端工作而设计"),
            Self::About => crate::i18n::t("关于 S3 Studio"),
            Self::TransferSettings => crate::i18n::t("设置"),
            Self::S3Defaults => crate::i18n::t("S3 全局默认值"),
            Self::CancelTransfer(_) => crate::i18n::t("取消传输？"),
            Self::Update => crate::i18n::t("软件更新"),
        }
    }
    pub fn submit_label(&self) -> &'static str {
        match self {
            Self::LocatePath => crate::i18n::t("定位路径"),
            Self::Account(_) => crate::i18n::t("保存账户"),
            Self::Folder => crate::i18n::t("创建文件夹"),
            Self::Bucket => crate::i18n::t("创建存储桶"),
            Self::Trash => crate::i18n::t("还原所选对象"),
            Self::Versions => crate::i18n::t("还原所选版本"),
            Self::Copy => crate::i18n::t("复制"),
            Self::Delete | Self::RemoveAccount(_) => crate::i18n::t("确认删除"),
            Self::Upload => crate::i18n::t("选择文件并上传"),
            Self::TransferSettings | Self::S3Defaults => crate::i18n::t("保存设置"),
            Self::Help => crate::i18n::t("开始使用"),
            Self::About => crate::i18n::t("关闭"),
            Self::CancelTransfer(_) => crate::i18n::t("取消并清理"),
            Self::Update => crate::i18n::t("检查更新"),
        }
    }
    pub fn destructive(&self) -> bool {
        matches!(
            self,
            Self::Delete | Self::RemoveAccount(_) | Self::CancelTransfer(_)
        )
    }
}
pub fn active(t: &Transfer) -> bool {
    matches!(
        t.status.as_str(),
        "queued" | "preparing" | "running" | "pausing" | "finalizing"
    )
}
pub fn status_label(status: &str) -> &'static str {
    match status {
        "queued" => crate::i18n::t("排队中"),
        "preparing" => crate::i18n::t("校验文件与断点"),
        "running" => crate::i18n::t("传输中"),
        "pausing" => crate::i18n::t("正在停止"),
        "paused" => crate::i18n::t("已暂停"),
        "finalizing" => crate::i18n::t("正在提交"),
        "done" => crate::i18n::t("已完成"),
        "error" => crate::i18n::t("失败，可重试"),
        "cancelled" => crate::i18n::t("已取消"),
        _ => "",
    }
}
pub fn object_name<'a>(key: &'a str, prefix: &str) -> &'a str {
    let name = key.strip_prefix(prefix).unwrap_or(key);
    let name = name.strip_suffix('/').unwrap_or(name);
    if name.is_empty() && key.ends_with('/') {
        "/"
    } else {
        name
    }
}
pub fn bytes(value: u128) -> String {
    if value == 0 {
        return "0 B".into();
    }
    let units = ["B", "KB", "MB", "GB", "TB", "PB", "EB", "ZB", "YB"];
    let unit = ((value as f64).log(1024.).floor() as usize).min(units.len() - 1);
    let number = if unit == 0 {
        value.to_string()
    } else {
        let scaled = value as f64 / 1024_f64.powi(unit as i32);
        format!("{:.1}", (scaled * 10.0).round() / 10.0)
            .trim_end_matches('0')
            .trim_end_matches('.')
            .to_owned()
    };
    let (integer, fraction) = number.split_once('.').unwrap_or((&number, ""));
    let mut grouped = String::new();
    for (i, c) in integer.chars().enumerate() {
        if i > 0 && (integer.len() - i) % 3 == 0 {
            grouped.push(',');
        }
        grouped.push(c);
    }
    if !fraction.is_empty() {
        grouped.push('.');
        grouped.push_str(fraction);
    }
    format!("{grouped} {}", units[unit])
}
pub fn date(value: &str) -> String {
    DateTime::parse_from_rfc3339(value)
        .map(|d| d.with_timezone(&Local).format("%m/%d %H:%M").to_string())
        .unwrap_or_else(|_| "—".into())
}
pub fn visible(objects: &[S3Object], query: &str, sort: Sort, ascending: bool) -> Vec<S3Object> {
    let query = query.to_lowercase();
    let mut result: Vec<_> = objects
        .iter()
        .filter(|o| o.key.to_lowercase().contains(&query))
        .cloned()
        .collect();
    result.sort_by(|a, b| {
        if a.folder != b.folder {
            return if a.folder {
                Ordering::Less
            } else {
                Ordering::Greater
            };
        }
        let order = match sort {
            Sort::Name => natural_cmp(&a.key, &b.key),
            Sort::Size => a.size.cmp(&b.size),
            Sort::Modified => a.modified.cmp(&b.modified),
        };
        if ascending { order } else { order.reverse() }
    });
    result
}
pub(crate) fn natural_cmp(a: &str, b: &str) -> Ordering {
    use icu_collator::{Collator, CollatorBorrowed};
    use std::sync::LazyLock;
    // Matches localeCompare(..., "zh-CN", { numeric: true }) in the original UI.
    static COLLATOR: LazyLock<Option<CollatorBorrowed<'static>>> = LazyLock::new(|| {
        Collator::try_new(
            icu_locale::locale!("zh-CN-u-kn-true").into(),
            Default::default(),
        )
        .ok()
    });
    COLLATOR
        .as_ref()
        .map_or_else(|| a.cmp(b), |collator| collator.compare(a, b))
}
