//! Подстановка `{{переменных}}` (FR-41, FR-44).
//!
//! Приоритет задаётся порядком слоёв: первый добавленный слой — самый сильный.
//! Для запроса вне коллекции: окружение → глобальные.

use crate::model::Variable;
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;
use uuid::Uuid;

#[derive(Default)]
pub struct Resolver<'a> {
    layers: Vec<&'a [Variable]>,
}

impl<'a> Resolver<'a> {
    pub fn new() -> Self {
        Self::default()
    }

    /// Добавляет слой с меньшим приоритетом, чем уже добавленные.
    pub fn with_layer(mut self, vars: &'a [Variable]) -> Self {
        self.layers.push(vars);
        self
    }

    /// Значение переменной или `None`, если она не определена.
    pub fn lookup(&self, name: &str) -> Option<String> {
        if let Some(dynamic_name) = name.strip_prefix('$') {
            return dynamic(dynamic_name);
        }
        self.layers.iter().find_map(|layer| {
            layer.iter().find(|v| v.enabled && v.key == name).map(|v| v.value.clone())
        })
    }

    /// Заменяет все известные `{{имя}}`. Неизвестные остаются как есть.
    pub fn resolve(&self, input: &str) -> String {
        let mut out = String::with_capacity(input.len());
        let mut rest = input;
        while let Some(start) = rest.find("{{") {
            out.push_str(&rest[..start]);
            let after = &rest[start + 2..];
            match after.find("}}") {
                Some(end) => {
                    let name = after[..end].trim();
                    match self.lookup(name) {
                        Some(value) => out.push_str(&value),
                        None => out.push_str(&rest[start..start + 2 + end + 2]),
                    }
                    rest = &after[end + 2..];
                }
                None => {
                    out.push_str(&rest[start..]);
                    rest = "";
                }
            }
        }
        out.push_str(rest);
        out
    }

    /// Имена переменных, которые не удалось разрешить (для подсветки, FR-42).
    pub fn unresolved(&self, input: &str) -> Vec<String> {
        let mut names = Vec::new();
        let mut rest = input;
        while let Some(start) = rest.find("{{") {
            let after = &rest[start + 2..];
            let Some(end) = after.find("}}") else { break };
            let name = after[..end].trim();
            if self.lookup(name).is_none() && !names.iter().any(|n| n == name) {
                names.push(name.to_owned());
            }
            rest = &after[end + 2..];
        }
        names
    }
}

fn dynamic(name: &str) -> Option<String> {
    match name {
        "uuid" | "guid" => Some(Uuid::new_v4().to_string()),
        "timestamp" => Some(OffsetDateTime::now_utc().unix_timestamp().to_string()),
        "isoTimestamp" => OffsetDateTime::now_utc().format(&Rfc3339).ok(),
        "randomInt" => Some((Uuid::new_v4().as_u128() % 1001).to_string()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn var(k: &str, v: &str) -> Variable {
        Variable::new(k, v)
    }

    #[test]
    fn environment_overrides_globals() {
        let env = [var("host", "dev.local")];
        let globals = [var("host", "prod"), var("port", "8080")];
        let r = Resolver::new().with_layer(&env).with_layer(&globals);
        assert_eq!(r.resolve("http://{{host}}:{{ port }}/"), "http://dev.local:8080/");
    }

    #[test]
    fn unknown_and_unclosed_are_kept() {
        let r = Resolver::new();
        assert_eq!(r.resolve("a {{nope}} b {{open"), "a {{nope}} b {{open");
        assert_eq!(r.unresolved("{{x}} {{x}} {{y}}"), vec!["x", "y"]);
    }

    #[test]
    fn disabled_variables_are_ignored() {
        let mut v = var("token", "secret");
        v.enabled = false;
        let vars = [v];
        let r = Resolver::new().with_layer(&vars);
        assert_eq!(r.resolve("{{token}}"), "{{token}}");
    }

    #[test]
    fn dynamic_variables() {
        let r = Resolver::new();
        assert_eq!(r.resolve("{{$uuid}}").len(), 36);
        assert!(r.resolve("{{$timestamp}}").parse::<i64>().is_ok());
        assert!(r.resolve("{{$randomInt}}").parse::<u32>().unwrap() <= 1000);
    }
}
