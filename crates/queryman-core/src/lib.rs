//! Ядро Queryman: модель данных, подстановка переменных и подготовка запроса.
//! Крейт не зависит от egui и не выполняет ввода-вывода.

pub mod model;
pub mod prepare;
pub mod protocol;
pub mod url_sync;
pub mod vars;

/// Имя приложения. Используется в заголовке окна, User-Agent и путях.
pub const APP_NAME: &str = "Queryman";
