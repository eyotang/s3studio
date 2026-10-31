use std::sync::atomic::{AtomicU8, Ordering};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Language { System, Chinese, English, Korean, Japanese, French, German, Russian }
impl Language {
    pub const ALL: [Self; 8] = [Self::System, Self::Chinese, Self::English, Self::Korean, Self::Japanese, Self::French, Self::German, Self::Russian];
    pub fn code(self) -> &'static str { match self {
        Self::System => "system", Self::Chinese => "zh-CN", Self::English => "en",
        Self::Korean => "ko", Self::Japanese => "ja", Self::French => "fr", Self::German => "de", Self::Russian => "ru",
    } }
    pub fn name(self) -> &'static str { match self {
        Self::System => t("跟随系统"), Self::Chinese => "简体中文", Self::English => "English",
        Self::Korean => "한국어", Self::Japanese => "日本語", Self::French => "Français", Self::German => "Deutsch", Self::Russian => "Русский",
    } }
    fn parse(code: &str) -> Self { Self::ALL.into_iter().find(|l| l.code() == code.trim()).unwrap_or(Self::System) }
}
static PREFERENCE: AtomicU8 = AtomicU8::new(Language::System as u8);
static CURRENT: AtomicU8 = AtomicU8::new(Language::Chinese as u8);
fn decode(value: u8) -> Language { Language::ALL.get(value as usize).copied().unwrap_or(Language::System) }
pub fn preference() -> Language { decode(PREFERENCE.load(Ordering::Relaxed)) }
pub fn current() -> Language { decode(CURRENT.load(Ordering::Relaxed)) }
pub fn t(text: &str) -> &str { translate(text, current()) }
pub fn resolve(language: Language, locale: &str) -> Language {
    if language != Language::System { return language; }
    let primary = locale.split(['-', '_', '.']).next().unwrap_or_default().to_ascii_lowercase();
    match primary.as_str() {
        "zh" => Language::Chinese, "ko" => Language::Korean, "ja" => Language::Japanese,
        "fr" => Language::French, "de" => Language::German, "ru" => Language::Russian, _ => Language::English,
    }
}
fn apply(language: Language) {
    PREFERENCE.store(language as u8, Ordering::Relaxed);
    CURRENT.store(resolve(language, &sys_locale::get_locale().unwrap_or_default()) as u8, Ordering::Relaxed);
}
include!(concat!(env!("OUT_DIR"), "/translations.rs"));

#[macro_export]
macro_rules! lf {
("已删除至少 {} 个对象，部分对象删除失败，请刷新列表确认：{}", $($args:tt)*) => { match $crate::i18n::current() {
$crate::i18n::Language::English => format!("Deleted at least {} objects; some deletions failed. Refresh the list to confirm: {}", $($args)*),
$crate::i18n::Language::Korean => format!("최소 {}개 객체를 삭제했지만 일부 삭제에 실패했습니다. 목록을 새로 고침하세요: {}", $($args)*),
$crate::i18n::Language::Japanese => format!("少なくとも{}個を削除しましたが、一部は失敗しました。一覧を更新して確認してください：{}", $($args)*),
$crate::i18n::Language::French => format!("Au moins {} objets supprimés ; certains ont échoué. Actualisez la liste : {}", $($args)*),
$crate::i18n::Language::German => format!("Mindestens {} Objekte gelöscht; einige Löschungen fehlgeschlagen. Liste aktualisieren: {}", $($args)*),
$crate::i18n::Language::Russian => format!("Удалено не менее {} объектов; некоторые не удалены. Обновите список: {}", $($args)*),
_ => format!("已删除至少 {} 个对象，部分对象删除失败，请刷新列表确认：{}", $($args)*), } };
("已完成 {} 个对象的删除，本批请求失败，请刷新列表确认：{}", $($args:tt)*) => { match $crate::i18n::current() {
$crate::i18n::Language::English => format!("Deleted {} objects; this batch failed. Refresh the list to confirm: {}", $($args)*),
$crate::i18n::Language::Korean => format!("{}개 객체 삭제 완료. 이번 요청이 실패했습니다. 목록을 새로 고침하세요: {}", $($args)*),
$crate::i18n::Language::Japanese => format!("{}個を削除しましたが、このリクエストは失敗しました。一覧を更新してください：{}", $($args)*),
$crate::i18n::Language::French => format!("{} objets supprimés ; cette requête a échoué. Actualisez la liste : {}", $($args)*),
$crate::i18n::Language::German => format!("{} Objekte gelöscht; diese Anfrage ist fehlgeschlagen. Liste aktualisieren: {}", $($args)*),
$crate::i18n::Language::Russian => format!("Удалено {} объектов; этот запрос завершился ошибкой. Обновите список: {}", $($args)*),
_ => format!("已完成 {} 个对象的删除，本批请求失败，请刷新列表确认：{}", $($args)*), } };
("{e}；凭据回滚失败：{rollback}", $($args:tt)*) => { match $crate::i18n::current() {
$crate::i18n::Language::English => format!("{e}; credential rollback failed: {rollback}", $($args)*),
$crate::i18n::Language::Korean => format!("{e}; 자격 증명 롤백 실패: {rollback}", $($args)*),
$crate::i18n::Language::Japanese => format!("{e}；認証情報のロールバックに失敗：{rollback}", $($args)*),
$crate::i18n::Language::French => format!("{e} ; échec de restauration des identifiants : {rollback}", $($args)*),
$crate::i18n::Language::German => format!("{e}; Wiederherstellen der Zugangsdaten fehlgeschlagen: {rollback}", $($args)*),
$crate::i18n::Language::Russian => format!("{e}; не удалось восстановить учётные данные: {rollback}", $($args)*),
_ => format!("{e}；凭据回滚失败：{rollback}", $($args)*), } };
("{error}；凭据回滚失败：{rollback_error}", $($args:tt)*) => { match $crate::i18n::current() {
$crate::i18n::Language::English => format!("{error}; credential rollback failed: {rollback_error}", $($args)*),
$crate::i18n::Language::Korean => format!("{error}; 자격 증명 롤백 실패: {rollback_error}", $($args)*),
$crate::i18n::Language::Japanese => format!("{error}；認証情報のロールバックに失敗：{rollback_error}", $($args)*),
$crate::i18n::Language::French => format!("{error} ; échec de restauration des identifiants : {rollback_error}", $($args)*),
$crate::i18n::Language::German => format!("{error}; Wiederherstellen der Zugangsdaten fehlgeschlagen: {rollback_error}", $($args)*),
$crate::i18n::Language::Russian => format!("{error}; не удалось восстановить учётные данные: {rollback_error}", $($args)*),
_ => format!("{error}；凭据回滚失败：{rollback_error}", $($args)*), } };
("系统密钥库操作失败：{e}。请确认密钥库已解锁；Ubuntu 需启用 Secret Service。", $($args:tt)*) => { match $crate::i18n::current() {
$crate::i18n::Language::English => format!("System keychain operation failed: {e}. Unlock your keychain; on Ubuntu, enable Secret Service.", $($args)*),
$crate::i18n::Language::Korean => format!("시스템 키체인 작업 실패: {e}. 키체인을 잠금 해제하고 Ubuntu에서는 Secret Service를 활성화하세요.", $($args)*),
$crate::i18n::Language::Japanese => format!("システムキーチェーンの操作に失敗：{e}。キーリングのロックを解除してください。UbuntuではSecret Serviceが必要です。", $($args)*),
$crate::i18n::Language::French => format!("Échec du trousseau système : {e}. Déverrouillez-le ; sous Ubuntu, activez Secret Service.", $($args)*),
$crate::i18n::Language::German => format!("Systemschlüsselbund fehlgeschlagen: {e}. Entsperren Sie ihn; aktivieren Sie unter Ubuntu Secret Service.", $($args)*),
$crate::i18n::Language::Russian => format!("Ошибка системной связки ключей: {e}. Разблокируйте её; в Ubuntu включите Secret Service.", $($args)*),
_ => format!("系统密钥库操作失败：{e}。请确认密钥库已解锁；Ubuntu 需启用 Secret Service。", $($args)*), } };
("无法替换账户配置：{e}", $($args:tt)*) => { match $crate::i18n::current() {
$crate::i18n::Language::English => format!("Unable to replace account settings: {e}", $($args)*),
$crate::i18n::Language::Korean => format!("계정 설정을 교체할 수 없습니다: {e}", $($args)*),
$crate::i18n::Language::Japanese => format!("アカウント設定を置き換えられません：{e}", $($args)*),
$crate::i18n::Language::French => format!("Impossible de remplacer les réglages du compte : {e}", $($args)*),
$crate::i18n::Language::German => format!("Kontoeinstellungen können nicht ersetzt werden: {e}", $($args)*),
$crate::i18n::Language::Russian => format!("Не удалось заменить настройки аккаунта: {e}", $($args)*),
_ => format!("无法替换账户配置：{e}", $($args)*), } };
("无法保存账户配置：{e}", $($args:tt)*) => { match $crate::i18n::current() {
$crate::i18n::Language::English => format!("Unable to save account settings: {e}", $($args)*),
$crate::i18n::Language::Korean => format!("계정 설정을 저장할 수 없습니다: {e}", $($args)*),
$crate::i18n::Language::Japanese => format!("アカウント設定を保存できません：{e}", $($args)*),
$crate::i18n::Language::French => format!("Impossible d’enregistrer les réglages du compte : {e}", $($args)*),
$crate::i18n::Language::German => format!("Kontoeinstellungen können nicht gespeichert werden: {e}", $($args)*),
$crate::i18n::Language::Russian => format!("Не удалось сохранить настройки аккаунта: {e}", $($args)*),
_ => format!("无法保存账户配置：{e}", $($args)*), } };
("{error}；分片清理失败：{}", $($args:tt)*) => { match $crate::i18n::current() {
$crate::i18n::Language::English => format!("{error}; failed to clean up parts: {}", $($args)*),
$crate::i18n::Language::Korean => format!("{error}; 파트 정리 실패: {}", $($args)*),
$crate::i18n::Language::Japanese => format!("{error}；パートのクリーンアップに失敗：{}", $($args)*),
$crate::i18n::Language::French => format!("{error} ; échec du nettoyage des parties : {}", $($args)*),
$crate::i18n::Language::German => format!("{error}; Bereinigen der Teile fehlgeschlagen: {}", $($args)*),
$crate::i18n::Language::Russian => format!("{error}; не удалось очистить части: {}", $($args)*),
_ => format!("{error}；分片清理失败：{}", $($args)*), } };
("无法保存传输断点：{e}", $($args:tt)*) => { match $crate::i18n::current() {
$crate::i18n::Language::English => format!("Unable to save transfer checkpoint: {e}", $($args)*),
$crate::i18n::Language::Korean => format!("전송 체크포인트를 저장할 수 없습니다: {e}", $($args)*),
$crate::i18n::Language::Japanese => format!("転送の再開情報を保存できません：{e}", $($args)*),
$crate::i18n::Language::French => format!("Impossible d’enregistrer le point de reprise : {e}", $($args)*),
$crate::i18n::Language::German => format!("Übertragungsprüfpunkt kann nicht gespeichert werden: {e}", $($args)*),
$crate::i18n::Language::Russian => format!("Не удалось сохранить точку возобновления: {e}", $($args)*),
_ => format!("无法保存传输断点：{e}", $($args)*), } };
("已添加 {count} 个上传任务，部分失败：{}", $($args:tt)*) => { match $crate::i18n::current() {
$crate::i18n::Language::English => format!("Added {count} uploads; some failed: {}", $($args)*),
$crate::i18n::Language::Korean => format!("업로드 {count}개 추가. 일부 실패: {}", $($args)*),
$crate::i18n::Language::Japanese => format!("アップロードを{count}件追加。一部失敗：{}", $($args)*),
$crate::i18n::Language::French => format!("{count} téléversements ajoutés ; certains ont échoué : {}", $($args)*),
$crate::i18n::Language::German => format!("{count} Uploads hinzugefügt; einige fehlgeschlagen: {}", $($args)*),
$crate::i18n::Language::Russian => format!("Добавлено загрузок: {count}; некоторые не удались: {}", $($args)*),
_ => format!("已添加 {count} 个上传任务，部分失败：{}", $($args)*), } };
("暂时无法检查更新：{error}", $($args:tt)*) => { match $crate::i18n::current() {
$crate::i18n::Language::English => format!("Unable to check for updates: {error}", $($args)*),
$crate::i18n::Language::Korean => format!("업데이트를 확인할 수 없습니다: {error}", $($args)*),
$crate::i18n::Language::Japanese => format!("更新を確認できません：{error}", $($args)*),
$crate::i18n::Language::French => format!("Impossible de rechercher les mises à jour : {error}", $($args)*),
$crate::i18n::Language::German => format!("Updates können nicht geprüft werden: {error}", $($args)*),
$crate::i18n::Language::Russian => format!("Не удалось проверить обновления: {error}", $($args)*),
_ => format!("暂时无法检查更新：{error}", $($args)*), } };
("更新未完成，当前版本保持不变：{error}", $($args:tt)*) => { match $crate::i18n::current() {
$crate::i18n::Language::English => format!("Update incomplete; the current version is unchanged: {error}", $($args)*),
$crate::i18n::Language::Korean => format!("업데이트 미완료. 현재 버전이 유지됩니다: {error}", $($args)*),
$crate::i18n::Language::Japanese => format!("更新は未完了です。現在のバージョンを保持します：{error}", $($args)*),
$crate::i18n::Language::French => format!("Mise à jour incomplète ; la version actuelle est conservée : {error}", $($args)*),
$crate::i18n::Language::German => format!("Update unvollständig; die aktuelle Version bleibt erhalten: {error}", $($args)*),
$crate::i18n::Language::Russian => format!("Обновление не завершено; текущая версия сохранена: {error}", $($args)*),
_ => format!("更新未完成，当前版本保持不变：{error}", $($args)*), } };
("未知操作：{command}", $($args:tt)*) => { match $crate::i18n::current() {
$crate::i18n::Language::English => format!("Unknown operation: {command}", $($args)*),
$crate::i18n::Language::Korean => format!("알 수 없는 작업: {command}", $($args)*),
$crate::i18n::Language::Japanese => format!("不明な操作：{command}", $($args)*),
$crate::i18n::Language::French => format!("Opération inconnue : {command}", $($args)*),
$crate::i18n::Language::German => format!("Unbekannter Vorgang: {command}", $($args)*),
$crate::i18n::Language::Russian => format!("Неизвестная операция: {command}", $($args)*),
_ => format!("未知操作：{command}", $($args)*), } };
("已删除 {} 个对象", $($args:tt)*) => { match $crate::i18n::current() {
$crate::i18n::Language::English => format!("Deleted {} objects", $($args)*),
$crate::i18n::Language::Korean => format!("객체 {}개 삭제됨", $($args)*),
$crate::i18n::Language::Japanese => format!("{}個のオブジェクトを削除しました", $($args)*),
$crate::i18n::Language::French => format!("{} objets supprimés", $($args)*),
$crate::i18n::Language::German => format!("{} Objekte gelöscht", $($args)*),
$crate::i18n::Language::Russian => format!("Удалено объектов: {}", $($args)*),
_ => format!("已删除 {} 个对象", $($args)*), } };
("已添加 {count} 个上传任务，可在传输活动中暂停或继续", $($args:tt)*) => { match $crate::i18n::current() {
$crate::i18n::Language::English => format!("Added {count} uploads; pause or resume them in Transfers", $($args)*),
$crate::i18n::Language::Korean => format!("업로드 {count}개 추가. 전송 활동에서 일시 중지하거나 재개할 수 있습니다", $($args)*),
$crate::i18n::Language::Japanese => format!("アップロードを{count}件追加しました。転送状況で一時停止・再開できます", $($args)*),
$crate::i18n::Language::French => format!("{count} téléversements ajoutés ; pause et reprise dans Transferts", $($args)*),
$crate::i18n::Language::German => format!("{count} Uploads hinzugefügt; in Übertragungen pausieren oder fortsetzen", $($args)*),
$crate::i18n::Language::Russian => format!("Добавлено загрузок: {count}; пауза и продолжение доступны в разделе передач", $($args)*),
_ => format!("已添加 {count} 个上传任务，可在传输活动中暂停或继续", $($args)*), } };
("无法启动 S3 Studio：{error}", $($args:tt)*) => { match $crate::i18n::current() {
$crate::i18n::Language::English => format!("Unable to start S3 Studio: {error}", $($args)*),
$crate::i18n::Language::Korean => format!("S3 Studio를 시작할 수 없습니다: {error}", $($args)*),
$crate::i18n::Language::Japanese => format!("S3 Studioを起動できません：{error}", $($args)*),
$crate::i18n::Language::French => format!("Impossible de démarrer S3 Studio : {error}", $($args)*),
$crate::i18n::Language::German => format!("S3 Studio kann nicht gestartet werden: {error}", $($args)*),
$crate::i18n::Language::Russian => format!("Не удалось запустить S3 Studio: {error}", $($args)*),
_ => format!("无法启动 S3 Studio：{error}", $($args)*), } };
("{active} 项正在传输", $($args:tt)*) => { match $crate::i18n::current() {
$crate::i18n::Language::English => format!("{active} active", $($args)*),
$crate::i18n::Language::Korean => format!("{active}개 전송 중", $($args)*),
$crate::i18n::Language::Japanese => format!("{active}件転送中", $($args)*),
$crate::i18n::Language::French => format!("{active} en cours", $($args)*),
$crate::i18n::Language::German => format!("{active} aktiv", $($args)*),
$crate::i18n::Language::Russian => format!("Активных: {active}", $($args)*),
_ => format!("{active} 项正在传输", $($args)*), } };
("{paused} 项已暂停", $($args:tt)*) => { match $crate::i18n::current() {
$crate::i18n::Language::English => format!("{paused} paused", $($args)*),
$crate::i18n::Language::Korean => format!("{paused}개 일시 중지", $($args)*),
$crate::i18n::Language::Japanese => format!("{paused}件一時停止", $($args)*),
$crate::i18n::Language::French => format!("{paused} en pause", $($args)*),
$crate::i18n::Language::German => format!("{paused} pausiert", $($args)*),
$crate::i18n::Language::Russian => format!("Приостановлено: {paused}", $($args)*),
_ => format!("{paused} 项已暂停", $($args)*), } };
("{} 路并发", $($args:tt)*) => { match $crate::i18n::current() {
$crate::i18n::Language::English => format!("{} concurrent", $($args)*),
$crate::i18n::Language::Korean => format!("{}개 동시 전송", $($args)*),
$crate::i18n::Language::Japanese => format!("{}並列", $($args)*),
$crate::i18n::Language::French => format!("{} simultanés", $($args)*),
$crate::i18n::Language::German => format!("{} parallel", $($args)*),
$crate::i18n::Language::Russian => format!("Параллельно: {}", $($args)*),
_ => format!("{} 路并发", $($args)*), } };
("暂停 {}", $($args:tt)*) => { match $crate::i18n::current() {
$crate::i18n::Language::English => format!("Pause {}", $($args)*),
$crate::i18n::Language::Korean => format!("{} 일시 중지", $($args)*),
$crate::i18n::Language::Japanese => format!("{}を一時停止", $($args)*),
$crate::i18n::Language::French => format!("Mettre {} en pause", $($args)*),
$crate::i18n::Language::German => format!("{} pausieren", $($args)*),
$crate::i18n::Language::Russian => format!("Приостановить {}", $($args)*),
_ => format!("暂停 {}", $($args)*), } };
("取消 {}", $($args:tt)*) => { match $crate::i18n::current() {
$crate::i18n::Language::English => format!("Cancel {}", $($args)*),
$crate::i18n::Language::Korean => format!("{} 취소", $($args)*),
$crate::i18n::Language::Japanese => format!("{}をキャンセル", $($args)*),
$crate::i18n::Language::French => format!("Annuler {}", $($args)*),
$crate::i18n::Language::German => format!("{} abbrechen", $($args)*),
$crate::i18n::Language::Russian => format!("Отменить {}", $($args)*),
_ => format!("取消 {}", $($args)*), } };
("{} 个文件{}", $($args:tt)*) => { match $crate::i18n::current() {
$crate::i18n::Language::English => format!("{} files{}", $($args)*),
$crate::i18n::Language::Korean => format!("파일 {}개{}", $($args)*),
$crate::i18n::Language::Japanese => format!("{}ファイル{}", $($args)*),
$crate::i18n::Language::French => format!("{} fichiers{}", $($args)*),
$crate::i18n::Language::German => format!("{} Dateien{}", $($args)*),
$crate::i18n::Language::Russian => format!("{} файлов{}", $($args)*),
_ => format!("{} 个文件{}", $($args)*), } };
("已选 {} 项", $($args:tt)*) => { match $crate::i18n::current() {
$crate::i18n::Language::English => format!("{} selected", $($args)*),
$crate::i18n::Language::Korean => format!("{}개 선택됨", $($args)*),
$crate::i18n::Language::Japanese => format!("{}件選択", $($args)*),
$crate::i18n::Language::French => format!("{} sélectionnés", $($args)*),
$crate::i18n::Language::German => format!("{} ausgewählt", $($args)*),
$crate::i18n::Language::Russian => format!("Выбрано: {}", $($args)*),
_ => format!("已选 {} 项", $($args)*), } };
("{} 个项目{}{}", $($args:tt)*) => { match $crate::i18n::current() {
$crate::i18n::Language::English => format!("{} items{}{}", $($args)*),
$crate::i18n::Language::Korean => format!("{}개 항목{}{}", $($args)*),
$crate::i18n::Language::Japanese => format!("{}項目{}{}", $($args)*),
$crate::i18n::Language::French => format!("{} éléments{}{}", $($args)*),
$crate::i18n::Language::German => format!("{} Elemente{}{}", $($args)*),
$crate::i18n::Language::Russian => format!("{} элементов{}{}", $($args)*),
_ => format!("{} 个项目{}{}", $($args)*), } };
(" · 已选择 {} 项", $($args:tt)*) => { match $crate::i18n::current() {
$crate::i18n::Language::English => format!(" · {} selected", $($args)*),
$crate::i18n::Language::Korean => format!(" · {}개 선택됨", $($args)*),
$crate::i18n::Language::Japanese => format!(" · {}件選択", $($args)*),
$crate::i18n::Language::French => format!(" · {} sélectionnés", $($args)*),
$crate::i18n::Language::German => format!(" · {} ausgewählt", $($args)*),
$crate::i18n::Language::Russian => format!(" · Выбрано: {}", $($args)*),
_ => format!(" · 已选择 {} 项", $($args)*), } };
("{} · 当前已加载", $($args:tt)*) => { match $crate::i18n::current() {
$crate::i18n::Language::English => format!("{} · Loaded so far", $($args)*),
$crate::i18n::Language::Korean => format!("{} · 현재 로드됨", $($args)*),
$crate::i18n::Language::Japanese => format!("{} · 読み込み済み", $($args)*),
$crate::i18n::Language::French => format!("{} · Chargés", $($args)*),
$crate::i18n::Language::German => format!("{} · Bisher geladen", $($args)*),
$crate::i18n::Language::Russian => format!("{} · Загружено", $($args)*),
_ => format!("{} · 当前已加载", $($args)*), } };
("选择 {}", $($args:tt)*) => { match $crate::i18n::current() {
$crate::i18n::Language::English => format!("Select {}", $($args)*),
$crate::i18n::Language::Korean => format!("{} 선택", $($args)*),
$crate::i18n::Language::Japanese => format!("{}を選択", $($args)*),
$crate::i18n::Language::French => format!("Sélectionner {}", $($args)*),
$crate::i18n::Language::German => format!("{} auswählen", $($args)*),
$crate::i18n::Language::Russian => format!("Выбрать {}", $($args)*),
_ => format!("选择 {}", $($args)*), } };
("查看 {}", $($args:tt)*) => { match $crate::i18n::current() {
$crate::i18n::Language::English => format!("View {}", $($args)*),
$crate::i18n::Language::Korean => format!("{} 보기", $($args)*),
$crate::i18n::Language::Japanese => format!("{}を表示", $($args)*),
$crate::i18n::Language::French => format!("Afficher {}", $($args)*),
$crate::i18n::Language::German => format!("{} anzeigen", $($args)*),
$crate::i18n::Language::Russian => format!("Открыть {}", $($args)*),
_ => format!("查看 {}", $($args)*), } };
("将递归删除 {folders} 个目录及其中所有文件和子目录，并删除 {} 个所选文件。未启用版本控制的对象无法恢复。", $($args:tt)*) => { match $crate::i18n::current() {
$crate::i18n::Language::English => format!("Recursively delete {folders} folders and all their contents, plus {} selected files. Objects without versioning cannot be recovered.", $($args)*),
$crate::i18n::Language::Korean => format!("폴더 {folders}개와 모든 내용을 재귀 삭제하고 선택한 파일 {}개를 삭제합니다. 버전 관리가 없는 객체는 복구할 수 없습니다.", $($args)*),
$crate::i18n::Language::Japanese => format!("{folders}個のフォルダーと全内容、および選択した{}個のファイルを削除します。バージョニングなしのオブジェクトは復元できません。", $($args)*),
$crate::i18n::Language::French => format!("Supprimer {folders} dossiers et leur contenu, ainsi que {} fichiers sélectionnés. Les objets sans versionnage sont irrécupérables.", $($args)*),
$crate::i18n::Language::German => format!("{folders} Ordner mit gesamtem Inhalt und {} ausgewählte Dateien rekursiv löschen. Objekte ohne Versionierung sind nicht wiederherstellbar.", $($args)*),
$crate::i18n::Language::Russian => format!("Будут удалены папки ({folders}) со всем содержимым и выбранные файлы ({}). Объекты без версионирования восстановить нельзя.", $($args)*),
_ => format!("将递归删除 {folders} 个目录及其中所有文件和子目录，并删除 {} 个所选文件。未启用版本控制的对象无法恢复。", $($args)*), } };
("即将删除 {} 个文件。未启用版本控制的对象无法恢复。", $($args:tt)*) => { match $crate::i18n::current() {
$crate::i18n::Language::English => format!("Delete {} files. Objects without versioning cannot be recovered.", $($args)*),
$crate::i18n::Language::Korean => format!("파일 {}개를 삭제합니다. 버전 관리가 없는 객체는 복구할 수 없습니다.", $($args)*),
$crate::i18n::Language::Japanese => format!("{}個のファイルを削除します。バージョニングなしのオブジェクトは復元できません。", $($args)*),
$crate::i18n::Language::French => format!("Supprimer {} fichiers. Les objets sans versionnage sont irrécupérables.", $($args)*),
$crate::i18n::Language::German => format!("{} Dateien löschen. Objekte ohne Versionierung sind nicht wiederherstellbar.", $($args)*),
$crate::i18n::Language::Russian => format!("Будет удалено файлов: {}. Объекты без версионирования восстановить нельзя.", $($args)*),
_ => format!("即将删除 {} 个文件。未启用版本控制的对象无法恢复。", $($args)*), } };
("{} 的断点将被清理；已经存在的源文件和目标文件不受影响。", $($args:tt)*) => { match $crate::i18n::current() {
$crate::i18n::Language::English => format!("The checkpoint for {} will be removed. Existing source and destination files are unaffected.", $($args)*),
$crate::i18n::Language::Korean => format!("{}의 체크포인트를 삭제합니다. 기존 원본 및 대상 파일에는 영향이 없습니다.", $($args)*),
$crate::i18n::Language::Japanese => format!("{}の再開情報を削除します。既存の元ファイルと保存先のファイルには影響しません。", $($args)*),
$crate::i18n::Language::French => format!("Le point de reprise de {} sera supprimé. Les fichiers source et de destination existants ne sont pas affectés.", $($args)*),
$crate::i18n::Language::German => format!("Der Prüfpunkt für {} wird entfernt. Bestehende Quell- und Zieldateien bleiben unverändert.", $($args)*),
$crate::i18n::Language::Russian => format!("Точка возобновления для {} будет удалена. Существующие исходные файлы и файлы назначения не затрагиваются.", $($args)*),
_ => format!("{} 的断点将被清理；已经存在的源文件和目标文件不受影响。", $($args)*), } };
("当前版本 {}{}", $($args:tt)*) => { match $crate::i18n::current() {
$crate::i18n::Language::English => format!("Current version {}{}", $($args)*),
$crate::i18n::Language::Korean => format!("현재 버전 {}{}", $($args)*),
$crate::i18n::Language::Japanese => format!("現在のバージョン {}{}", $($args)*),
$crate::i18n::Language::French => format!("Version actuelle {}{}", $($args)*),
$crate::i18n::Language::German => format!("Aktuelle Version {}{}", $($args)*),
$crate::i18n::Language::Russian => format!("Текущая версия {}{}", $($args)*),
_ => format!("当前版本 {}{}", $($args)*), } };
("版本 {}", $($args:tt)*) => { match $crate::i18n::current() {
$crate::i18n::Language::English => format!("Version {}", $($args)*),
$crate::i18n::Language::Korean => format!("버전 {}", $($args)*),
$crate::i18n::Language::Japanese => format!("バージョン {}", $($args)*),
$crate::i18n::Language::French => format!("Version {}", $($args)*),
$crate::i18n::Language::German => format!("Version {}", $($args)*),
$crate::i18n::Language::Russian => format!("Версия {}", $($args)*),
_ => format!("版本 {}", $($args)*), } };
("上次检查：{}", $($args:tt)*) => { match $crate::i18n::current() {
$crate::i18n::Language::English => format!("Last checked: {}", $($args)*),
$crate::i18n::Language::Korean => format!("마지막 확인: {}", $($args)*),
$crate::i18n::Language::Japanese => format!("最終確認：{}", $($args)*),
$crate::i18n::Language::French => format!("Dernière vérification : {}", $($args)*),
$crate::i18n::Language::German => format!("Zuletzt geprüft: {}", $($args)*),
$crate::i18n::Language::Russian => format!("Последняя проверка: {}", $($args)*),
_ => format!("上次检查：{}", $($args)*), } };
}

fn settings_path() -> std::io::Result<std::path::PathBuf> {
    dirs::config_dir()
        .map(|p| p.join("com.s3studio.desktop").join("language"))
        .ok_or_else(|| std::io::Error::other("Unable to locate settings directory"))
}
pub fn initialize() {
    let saved = settings_path()
        .ok()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .unwrap_or_default();
    apply(Language::parse(&saved));
}
pub fn select(language: Language) -> std::io::Result<()> {
    let path = settings_path()?;
    save_preference(&path, language)?;
    apply(language);
    Ok(())
}

pub fn translate_placeholder(text: &str) -> String {
    let source = placeholder_source(text);
    t(source).to_owned()
}

fn save_preference(path: &std::path::Path, language: Language) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let temporary = path.with_extension("tmp");
    std::fs::write(&temporary, language.code())?;
    std::fs::rename(temporary, path)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn detects_all_supported_system_languages_and_honors_overrides() {
        for (locale, expected) in [("zh_TW.UTF-8", Language::Chinese), ("ZH-Hans", Language::Chinese), ("ko-KR", Language::Korean), ("ja_JP.UTF-8", Language::Japanese), ("fr-CA", Language::French), ("de-DE", Language::German), ("ru-RU", Language::Russian), ("es-ES", Language::English), ("", Language::English)] {
            assert_eq!(resolve(Language::System, locale), expected);
            assert_eq!(resolve(Language::French, locale), Language::French);
        }
    }
    #[test]
    fn catalogs_cover_messages_and_preserve_user_content() {
        for (language, settings) in [(Language::English, "Settings"), (Language::Korean, "설정"), (Language::Japanese, "設定"), (Language::French, "Réglages"), (Language::German, "Einstellungen"), (Language::Russian, "Настройки")] {
            assert_eq!(translate("设置", language), settings);
            assert_eq!(translate("用户文件-{n}.txt", language), "用户文件-{n}.txt");
            let placeholder = translate("例如：生产环境 / 个人存储", language);
            assert_eq!(placeholder_source(placeholder), "例如：生产环境 / 个人存储");
        }
        assert_eq!(translate("设置", Language::Chinese), "设置");
    }
    #[test]
    fn language_preferences_round_trip() {
        let dir = std::env::temp_dir().join(format!("s3-languages-{}", uuid::Uuid::new_v4()));
        let path = dir.join("language");
        for language in Language::ALL {
            save_preference(&path, language).unwrap();
            assert_eq!(Language::parse(&std::fs::read_to_string(&path).unwrap()), language);
        }
        assert_eq!(Language::parse("unknown"), Language::System);
        std::fs::remove_dir_all(dir).unwrap();
    }
    // Execute separately: UI and worker threads intentionally share the active locale.
    #[test]
    #[ignore = "Run separately: mutates the process-wide language"]
    fn multilingual_runtime_smoke() {
        let count = 3;
        for language in Language::ALL.into_iter().filter(|l| *l != Language::System) {
            apply(language);
            let expected = match language {
                Language::Chinese => "已删除 3 个对象",
                Language::English => "Deleted 3 objects",
                Language::Korean => "객체 3개 삭제됨",
                Language::Japanese => "3個のオブジェクトを削除しました",
                Language::French => "3 objets supprimés",
                Language::German => "3 Objekte gelöscht",
                Language::Russian => "Удалено объектов: 3",
                Language::System => unreachable!(),
            };
            assert_eq!(crate::lf!("已删除 {} 个对象", count), expected);
            let message = crate::lf!("已添加 {count} 个上传任务，可在传输活动中暂停或继续", count = count);
            assert!(message.contains('3'));
            let name = "report-{id}-日本語.txt";
            assert!(crate::lf!("选择 {}", name).contains(name));
            let folders = 2;
            let confirmation = crate::lf!("将递归删除 {folders} 个目录及其中所有文件和子目录，并删除 {} 个所选文件。未启用版本控制的对象无法恢复。", count, folders = folders);
            assert!(confirmation.contains('2') && confirmation.contains('3'));
            assert!(!confirmation.contains("{folders}"));
            assert!(!message.contains("{count}"));
        }
        apply(Language::System);
    }
}
