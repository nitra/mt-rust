//! Discovery / single-instance (спека runtime.md, «agent-server — один
//! хост-процес на машину»).
//!
//! Сервер пише port-file (`server.port`: port + pid + sha256-хеш токена)
//! і тримає lock-файл; сирий токен — у `server.token` (права 0600), його
//! читає лише той самий користувач (тонкий клієнт на цій машині). Перевірка
//! «живий чи stale» — обовʼязок клієнта: пробний `ClientHello` (спека);
//! stale lock перезаписується.

use std::fs;
use std::io;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// sha256-хеш токена у hex — для port-file (сирий токен туди не пишеться).
pub fn token_hash(token: &str) -> String {
    let digest = Sha256::digest(token.as_bytes());
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Вміст `server.port`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PortFile {
    pub port: u16,
    pub pid: u32,
    pub token_hash: String,
}

/// Файлова discovery-точка в конфігурованій директорії
/// (продакшн — `~/.nitra`, тести — tempdir).
pub struct Discovery {
    pub dir: PathBuf,
}

impl Discovery {
    pub fn new(dir: PathBuf) -> Self {
        Self { dir }
    }

    fn port_path(&self) -> PathBuf {
        self.dir.join("server.port")
    }

    fn token_path(&self) -> PathBuf {
        self.dir.join("server.token")
    }

    fn lock_path(&self) -> PathBuf {
        self.dir.join("server.lock")
    }

    /// Пише port-file + token-файл (0600) + lock. Наявний lock
    /// перезаписується — living-перевірку робить клієнт через ClientHello.
    pub fn write(&self, port: u16, token: &str) -> io::Result<()> {
        fs::create_dir_all(&self.dir)?;
        let port_file = PortFile {
            port,
            pid: std::process::id(),
            token_hash: token_hash(token),
        };
        fs::write(self.port_path(), serde_json::to_string_pretty(&port_file)?)?;
        fs::write(self.token_path(), token)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(self.token_path(), fs::Permissions::from_mode(0o600))?;
        }
        fs::write(self.lock_path(), std::process::id().to_string())?;
        Ok(())
    }

    /// Читає port-file і сирий токен; звіряє хеш (порушення → помилка —
    /// хтось підмінив один із файлів).
    pub fn read(&self) -> io::Result<(PortFile, String)> {
        let port_file: PortFile = serde_json::from_str(&fs::read_to_string(self.port_path())?)?;
        let token = fs::read_to_string(self.token_path())?;
        if token_hash(&token) != port_file.token_hash {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "token hash mismatch between server.port and server.token",
            ));
        }
        Ok((port_file, token))
    }

    /// Прибирає discovery-файли (акуратне завершення сервера).
    pub fn remove(&self) -> io::Result<()> {
        for path in [self.port_path(), self.token_path(), self.lock_path()] {
            match fs::remove_file(path) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(error),
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Запис → читання: порт/pid/хеш збігаються, токен звіряється хешем.
    #[test]
    fn write_then_read_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let discovery = Discovery::new(dir.path().to_path_buf());
        discovery.write(4123, "secret-token").unwrap();

        let (port_file, token) = discovery.read().unwrap();
        assert_eq!(port_file.port, 4123);
        assert_eq!(port_file.pid, std::process::id());
        assert_eq!(token, "secret-token");
        assert_eq!(port_file.token_hash, token_hash("secret-token"));
    }

    /// Підмінений токен → явна помилка, не тихе підключення.
    #[test]
    fn tampered_token_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let discovery = Discovery::new(dir.path().to_path_buf());
        discovery.write(4123, "secret-token").unwrap();
        std::fs::write(dir.path().join("server.token"), "інший").unwrap();

        let error = discovery.read().unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    }

    /// remove ідемпотентний.
    #[test]
    fn remove_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let discovery = Discovery::new(dir.path().to_path_buf());
        discovery.write(1, "t").unwrap();
        discovery.remove().unwrap();
        discovery.remove().unwrap();
        assert!(discovery.read().is_err());
    }
}
