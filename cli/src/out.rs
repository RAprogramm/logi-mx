// SPDX-FileCopyrightText: 2025-2026 RAprogramm <andrey.rozanov.vl@gmail.com>
// SPDX-License-Identifier: MIT

//! Buffered stdout writer shared by every command renderer.

use std::io::Write as IoWrite;

use logi_mx_driver::prelude::*;
use masterror::AppError;

/// Buffered stdout writer that maps I/O failures to [`AppError`].
pub struct Out(std::io::BufWriter<std::io::StdoutLock<'static>>);

impl Out {
    /// Locks stdout and wraps it in a buffered writer.
    pub fn new() -> Self {
        Self(std::io::BufWriter::new(std::io::stdout().lock()))
    }

    /// Writes one newline-terminated line.
    pub fn line(&mut self, args: std::fmt::Arguments<'_>) -> Result<()> {
        writeln!(self.0, "{args}")
            .map_err(|e| AppError::internal("Failed to write output").with_source(e))
    }

    /// Flushes buffered bytes to the terminal.
    pub fn finish(&mut self) -> Result<()> {
        self.0
            .flush()
            .map_err(|e| AppError::internal("Failed to flush output").with_source(e))
    }
}
