use core::fmt;

pub struct Error {
    title: String,
    line: Option<(String, usize, std::ops::Range<usize>)>,
}

impl Error {
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            line: None,
        }
    }

    pub fn at(mut self, src: &str, span: std::ops::Range<usize>) -> Self {
        let start_of_line = seek(src, span.start, |i| i - 1) + 1;
        let end_of_line = seek(src, span.end, |i| i + 1);

        let mut linenr = 1;
        for (i, &b) in src.as_bytes().iter().enumerate() {
            if b == b'\n' {
                linenr += 1;
            }
            if i >= start_of_line {
                break;
            }
        }

        fn seek(src: &str, mut i: usize, f: impl Fn(usize) -> usize) -> usize {
            loop {
                match src.as_bytes().get(i) {
                    None | Some(b'\n') => break i,
                    Some(_) if i == 0 => break 0,
                    Some(_) => i = f(i),
                }
            }
        }

        let line = &src[start_of_line..end_of_line];
        let arrow = span.start.checked_sub(start_of_line).unwrap_or(0)..span.end - start_of_line;
        self.line = Some((line.to_string(), linenr, arrow));

        self
    }

    pub fn emit(&self) {
        eprintln!("{self}")
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "error: {}", self.title)?;
        if let Some((line, linenr, arrow)) = &self.line {
            let mut arrow = arrow.clone();
            write!(f, "\n{linenr}: {line}\n")?;

            arrow.start += linenr.to_string().len() + 2;
            arrow.end += linenr.to_string().len() + 2;

            let mut size = arrow.end - arrow.start;
            while arrow.start > 0 {
                ' '.fmt(f)?;
                arrow.start -= 1;
            }
            while size > 0 {
                '^'.fmt(f)?;
                size -= 1;
            }
        }
        Ok(())
    }
}
