use logos::{Lexer, Logos};

fn find_char_end<'src>(end: u8, lex: &mut Lexer<'src, Token>) -> Option<char> {
    let mut i = 0;
    let bytes = lex.remainder().as_bytes();

    while let Some(&c) = bytes.get(i) {
        match c {
            b'\\' if bytes.get(i + 1) == Some(&end) => i += 2,
            b'\\' if bytes.get(i + 1) == Some(&b'\\') => i += 2,
            c if c == end => break,
            _ => i += 1,
        }
    }

    lex.bump(i + 1);

    match &bytes[..i] {
        b"\\n" => Some('\n'),
        b"\\t" => Some('\t'),
        b"\\'" => Some('\''),
        b"\\\\" => Some('\\'),
        bytes => std::str::from_utf8(&bytes)
            .ok()
            .and_then(|str| str.parse::<char>().ok()),
    }
}

fn split_field<'src>(lex: &mut Lexer<'src, Token>) -> (String, String) {
    let mut iter = lex.slice().split('.');
    let lhs = iter.next().unwrap();
    let rhs = iter.next().unwrap();
    (lhs.to_string(), rhs.to_string())
}

#[derive(Logos, Clone, Debug, PartialEq, Eq)]
#[logos(skip r"[ \t\n\f]+")]
pub enum Token {
    #[token("type")]
    Type,
    #[token("func")]
    Func,
    #[token("var")]
    Var,
    #[regex("[_a-zA-Z][-_0-9a-zA-Z]*", |lexer| lexer.slice().to_string())]
    Name(String),
    #[token(":")]
    Colon,
    #[token(";")]
    SemiColon,
    #[token(",")]
    Comma,
    #[token("'", |lex| find_char_end(b'\'', lex))]
    CharLiteral(char),
    #[token("return")]
    Return,
    #[token("->")]
    Arrow,
    #[regex("[\\d]+", |lexer| lexer.slice().parse::<i128>().unwrap())]
    Int(i128),
    #[token("null")]
    Null,
    #[token("when")]
    When,

    #[token("{")]
    CurlyOpen,
    #[token("}")]
    CurlyClose,
    #[token("(")]
    ParenOpen,
    #[token(")")]
    ParenClose,
    #[regex(r"[*\/\-+=]+", |lexer| lexer.slice().to_string())]
    Operator(String),
    #[regex("[_a-zA-Z][-_0-9a-zA-Z]*\\.[_a-zA-Z][-_0-9a-zA-Z]*", split_field)]
    FieldOf((String, String)),

    #[regex("#[^\n]*", logos::skip, allow_greedy = true)]
    #[allow(unused)]
    Comment,
}

impl Token {
    pub fn describe(&self) -> String {
        match self {
            Token::When => "`when` keyword".into(),
            Token::Null => "null".into(),
            Token::Int(_) => "number literal".into(),
            Token::Arrow => "`->`".into(),
            Token::Return => "`return` keyword".into(),
            Token::Type => "`type` keyword".into(),
            Token::Func => "`func` keyword".into(),
            Token::Var => "`var` keyword".into(),
            Token::Name(name) => format!("the identifier `{name}`"),
            Token::Colon => "`:`".into(),
            Token::SemiColon => "`;`".into(),
            Token::Comma => "`,`".into(),
            Token::CurlyOpen => "`{`".into(),
            Token::CurlyClose => "`}`".into(),
            Token::ParenOpen => "`(`".into(),
            Token::ParenClose => "`)`".into(),
            Token::CharLiteral(char) => format!("character `{char}`"),
            Token::Operator(name) => format!("the operator `{name}`"),
            Token::FieldOf((a, b)) => format!("the expression `{a}.{b}`"),
            Token::Comment => "a comment".into(),
        }
    }
}
