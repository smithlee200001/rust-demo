//! 文件日志：单流顺序写入，脱敏先于落盘或加密。
use age::{Encryptor, secrecy::SecretString, x25519};
use std::{
    borrow::Cow,
    fs::{File, OpenOptions},
    io::{self, BufWriter, Write},
    path::Path,
};

/// 文件格式及其所需材料。对称模式只收口令；非对称写入只需要公开收件人。
pub enum Mode {
    Plain,
    Symmetric(SecretString),
    Asymmetric(x25519::Recipient),
}

type BufferedFile = BufWriter<File>;

enum Sink {
    Plain(BufferedFile),
    Encrypted(Box<age::stream::StreamWriter<BufferedFile>>),
}

/// 每次运行创建一个文件；调用者负责串行写入并显式完成文件。
///
/// 不保存明文临时文件，不追加至已有文件。加密流必须调用 `finish`
/// 写入末块；进程被强制终止时可能只留下不完整的文件。
pub struct FileLog {
    sink: Sink,
    redact: bool,
}

impl FileLog {
    /// 创建一个新日志文件。目标已存在、密钥初始化或 I/O 失败均返回错误，
    /// 不回退到明文；在 Unix 上请求仅所有者可读写的权限。
    pub fn create(path: &Path, mode: Mode, redact: bool) -> io::Result<Self> {
        let encryptor = match mode {
            Mode::Plain => None,
            Mode::Symmetric(passphrase) => {
                let recipient = age::scrypt::Recipient::new(passphrase);
                Some(
                    Encryptor::with_recipients(std::iter::once(&recipient as &dyn age::Recipient))
                        .map_err(io::Error::other)?,
                )
            }
            Mode::Asymmetric(recipient) => Some(
                Encryptor::with_recipients(std::iter::once(&recipient as &dyn age::Recipient))
                    .map_err(io::Error::other)?,
            ),
        };
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let output = BufWriter::with_capacity(64 * 1024, options.open(path)?);
        let sink = if let Some(encryptor) = encryptor {
            Sink::Encrypted(Box::new(encryptor.wrap_output(output)?))
        } else {
            Sink::Plain(output)
        };
        Ok(Self { sink, redact })
    }

    /// 将已格式化的一条记录写入当前文件。识别的用户名在文件层统一掩码，
    /// 嵌入 CR/LF 变成可见转义以免伪造新的记录行。
    pub fn append(&mut self, line: &str) -> io::Result<()> {
        let record = if self.redact {
            Cow::Owned(crate::redact::redact(line))
        } else {
            Cow::Borrowed(line)
        };
        let safe = if record.contains(['\r', '\n']) {
            Cow::Owned(record.replace('\r', "\\r").replace('\n', "\\n"))
        } else {
            record
        };
        let writer = self.writer();
        writer.write_all(safe.as_bytes())?;
        writer.write_all(b"\n")
    }

    /// 刷新现有缓冲；不会结束加密流，因而不保证部分加密块已经写入磁盘。
    pub fn flush(&mut self) -> io::Result<()> {
        self.writer().flush()
    }

    /// 完成加密末块并将文件缓冲交给操作系统。不会执行 fsync；
    /// 任何完成/刷新错误都必须由调用者处理。
    pub fn finish(self) -> io::Result<()> {
        let mut file = match self.sink {
            Sink::Plain(writer) => writer,
            Sink::Encrypted(writer) => writer.finish()?,
        };
        file.flush()
    }

    fn writer(&mut self) -> &mut dyn Write {
        match &mut self.sink {
            Sink::Plain(writer) => writer,
            Sink::Encrypted(writer) => writer.as_mut(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, io::Read};

    #[test]
    fn plain_masks_optional_and_never_overwrites() -> io::Result<()> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("demo.log");
        let mut log = FileLog::create(&path, Mode::Plain, true)?;
        log.append("open /home/alice/data\r\nnext")?;
        log.finish()?;
        assert_eq!(
            fs::read_to_string(&path)?,
            "open /home/***/data\\r\\nnext\n"
        );
        assert_eq!(
            FileLog::create(&path, Mode::Plain, false)
                .err()
                .map(|e| e.kind()),
            Some(io::ErrorKind::AlreadyExists)
        );
        assert_eq!(
            fs::read_to_string(&path)?,
            "open /home/***/data\\r\\nnext\n"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(fs::metadata(&path)?.permissions().mode() & 0o077, 0);
        }
        Ok(())
    }

    #[test]
    fn both_encrypted_modes_round_trip_redaction() -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let key = x25519::Identity::generate();
        let passphrase = SecretString::from("synthetic test phrase only".to_owned());
        let pass_identity = age::scrypt::Identity::new(passphrase.clone());
        for (name, mode, identity) in [
            (
                "recipient",
                Mode::Asymmetric(key.to_public()),
                &key as &dyn age::Identity,
            ),
            (
                "passphrase",
                Mode::Symmetric(passphrase),
                &pass_identity as &dyn age::Identity,
            ),
        ] {
            let path = dir.path().join(name);
            let mut log = FileLog::create(&path, mode, true)?;
            log.append("fixture at /Users/alice/files")?;
            log.finish()?;
            let encrypted = fs::read(path)?;
            assert!(encrypted.starts_with(b"age-encryption.org/v1\n"));
            assert!(!encrypted.windows(b"alice".len()).any(|w| w == b"alice"));
            let mut clear = String::new();
            age::Decryptor::new(&encrypted[..])?
                .decrypt(std::iter::once(identity))?
                .read_to_string(&mut clear)?;
            assert_eq!(clear, "fixture at /Users/***/files\n");
        }
        Ok(())
    }

    #[test]
    fn wrong_key_and_modified_final_chunk_are_rejected() -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("demo.age");
        let key = x25519::Identity::generate();
        let mut log = FileLog::create(&path, Mode::Asymmetric(key.to_public()), false)?;
        log.append("a synthetic log")?;
        log.finish()?;
        let mut bytes = fs::read(path)?;
        let wrong = x25519::Identity::generate();
        assert!(
            age::Decryptor::new(&bytes[..])?
                .decrypt(std::iter::once(&wrong as &dyn age::Identity))
                .is_err()
        );
        *bytes.last_mut().ok_or("Missing age stream")? ^= 1;
        assert!(
            age::Decryptor::new(&bytes[..])?
                .decrypt(std::iter::once(&key as &dyn age::Identity))?
                .read_to_end(&mut Vec::new())
                .is_err()
        );
        Ok(())
    }
}
