//! Cliente do console de texto do Android Emulator (telnet em 127.0.0.1:<porta>).
use anyhow::{bail, Context, Result};
use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::time::Duration;

pub struct Console {
    reader: BufReader<TcpStream>,
    writer: TcpStream,
}

impl Console {
    /// Conecta e autentica com o token de `~/.emulator_console_auth_token`.
    pub fn connect(port: u16) -> Result<Self> {
        let s = TcpStream::connect(("127.0.0.1", port)).with_context(|| format!("console do emulador na porta {port}"))?;
        s.set_read_timeout(Some(Duration::from_secs(5)))?;
        let mut c = Self { reader: BufReader::new(s.try_clone()?), writer: s };
        c.read_reply()?;
        let token = std::fs::read_to_string(emuzip_core::home().join(".emulator_console_auth_token")).unwrap_or_default();
        if !token.trim().is_empty() {
            c.cmd(&format!("auth {}", token.trim()))?;
        }
        Ok(c)
    }

    fn read_reply(&mut self) -> Result<String> {
        let mut out = String::new();
        loop {
            let mut line = String::new();
            if self.reader.read_line(&mut line)? == 0 {
                bail!("console fechado");
            }
            let t = line.trim_end();
            if t == "OK" {
                return Ok(out);
            }
            if let Some(e) = t.strip_prefix("KO") {
                bail!("console: {}", e.trim_start_matches(':').trim());
            }
            out.push_str(t);
            out.push('\n');
        }
    }

    pub fn cmd(&mut self, c: &str) -> Result<String> {
        self.writer.write_all(format!("{c}\n").as_bytes())?;
        self.read_reply()
    }
}

/// Atalho: abre, executa e fecha.
pub fn run(port: u16, c: &str) -> Result<String> {
    Console::connect(port)?.cmd(c)
}
