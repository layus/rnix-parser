use crate::value::{Anchor, Value};
use std::mem;

#[derive(Debug, PartialEq, Eq)]
enum IdentType {
    Uri,
    Path,
    Ident
}

#[derive(Clone, Debug, PartialEq)]
pub enum Interpol {
    Literal(String),
    Tokens(Vec<(Meta, Token)>)
}

#[derive(Clone, Debug, PartialEq)]
pub enum Token {
    CurlyBOpen,
    CurlyBClose,
    Equal,
    Semicolon,
    Dot,
    Ident(String),
    Value(Value),
    Interpol(Vec<Interpol>),
    Let,
    In,
    With,
    Import,
    Rec,
    SquareBOpen,
    SquareBClose,
    Concat,

    ParenOpen,
    ParenClose,
    Add,
    Sub,
    Mul,
    Div
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Meta {
    pub span: Span,
    pub comments: Vec<String>
}
impl Meta {
    pub fn until(mut self, other: &Meta) -> Meta {
        self.span = self.span.until(other.span);
        self
    }
}
impl From<Span> for Meta {
    fn from(span: Span) -> Meta {
        Meta { span, ..Default::default() }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Span {
    pub start: (u64, u64),
    pub end: Option<(u64, u64)>
}
impl Span {
    pub fn until(self, other: Span) -> Span {
        Span {
            start: self.start,
            end: other.end
        }
    }
}

#[derive(Clone, Copy, Debug, Fail, PartialEq)]
pub enum TokenizeError {
    #[fail(display = "error parsing integer: overflow")]
    IntegerOverflow,
    #[fail(display = "dot after number, but no decimals")]
    TrailingDecimal,
    #[fail(display = "unexpected eof")]
    UnexpectedEOF,
    #[fail(display = "undefined token")]
    UndefinedToken,
    #[fail(display = "paths cannot have a trailing slash")]
    TrailingSlash,
    #[fail(display = "unclosed multiline comment")]
    UnclosedComment,
}

fn is_valid_path_char(c: char) -> bool {
    match c {
        'a'..='z' | 'A'..='Z' | '0'..='9' | '/' | '_' | '.' | '+' | '-' => true,
        _ => false
    }
}

type Item = Result<(Meta, Token), (Span, TokenizeError)>;

pub struct Tokenizer<'a> {
    input: &'a str,
    row: u64,
    col: u64
}
impl<'a> Tokenizer<'a> {
    pub fn new(input: &'a str) -> Self {
        Self {
            input,
            row: 0,
            col: 0
        }
    }

    fn span_start(&mut self) -> Span {
        Span {
            start: (self.row, self.col),
            end: None
        }
    }
    fn span_err(&self, meta: Meta, error: TokenizeError) -> Option<Item> {
        Some(Err((meta.span, error)))
    }
    fn span_end(&self, mut meta: Meta, token: Token) -> Option<Item> {
        meta.span.end = Some((self.row, self.col));
        Some(Ok((meta, token)))
    }

    fn next(&mut self) -> Option<char> {
        let c = self.peek();
        if let Some(c) = c {
            self.input = &self.input[c.len_utf8()..];
            if c == '\n' {
                self.col = 0;
                self.row += 1;
            } else {
                self.col += 1;
            }
        }
        c
    }
    fn peek(&self) -> Option<char> {
        self.input.chars().next()
    }

    fn next_ident<F>(&mut self, prefix: Option<char>, include: F) -> String
        where F: Fn(char) -> bool
    {
        let capacity = self.input.chars().take_while(|&c| include(c)).count()
            + if prefix.is_some() { 1 } else { 0 };
        let mut ident = String::with_capacity(capacity);
        let initial_pointer = ident.as_ptr();
        if let Some(c) = prefix {
            ident.push(c);
        }
        loop {
            match self.peek() {
                Some(c) if include(c) => ident.push(self.next().unwrap()),
                _ => break,
            }
        }
        assert_eq!(ident.as_ptr(), initial_pointer, "String reallocated, wasn't given enough capacity");
        ident
    }
    fn next_string(&mut self, meta: Meta, multiline: bool) -> Option<Item> {
        let mut interpol = Vec::new();
        let mut literal = String::new();
        loop {
            match self.peek() {
                None => return self.span_err(meta, TokenizeError::UnexpectedEOF),
                Some('"') if !multiline => { self.next(); break },
                Some('\'') if multiline => match { self.next()?; self.peek() } {
                    None => return self.span_err(meta, TokenizeError::UnexpectedEOF),
                    Some('\'') => { self.next()?; break; },
                    Some(_) => literal.push('\''),
                },
                Some('\n') if multiline => {
                    // Don't push initial newline
                    self.next()?;
                    if !literal.is_empty() {
                        literal.push('\n');
                    }
                    while self.peek() == Some(' ')
                            || self.peek() == Some('\t') {
                        self.next();
                    }
                },
                Some('\\') if !multiline => {
                    self.next()?;
                    literal.push(self.next()?);
                },
                Some('$') => match { self.next(); self.peek() } {
                    Some('{') => {
                        self.next()?;
                        interpol.push(Interpol::Literal(mem::replace(&mut literal, String::new())));

                        let mut tokens = Vec::new();
                        let mut count = 0;
                        loop {
                            match Iterator::next(self) {
                                None => return self.span_err(meta, TokenizeError::UnexpectedEOF),
                                Some(token) => {
                                    let token = match token {
                                        Ok(inner) => inner,
                                        result @ Err(_) => return Some(result)
                                    };
                                    match token.1 {
                                        Token::CurlyBOpen => count += 1,
                                        Token::CurlyBClose if count == 0 => break,
                                        Token::CurlyBClose => count -= 1,
                                        _ => ()
                                    }
                                    tokens.push(token);
                                }
                            }
                        }

                        interpol.push(Interpol::Tokens(tokens));
                    },
                    _ => literal.push('$')
                }
                Some(_) => {
                    literal.push(self.next()?);
                }
            }
        }

        if interpol.is_empty() {
            self.span_end(meta, Token::Value(Value::Str(literal)))
        } else {
            if !literal.is_empty() {
                interpol.push(Interpol::Literal(literal));
            }
            self.span_end(meta, Token::Interpol(interpol))
        }
    }
}
impl<'a> Iterator for Tokenizer<'a> {
    type Item = Item;

    fn next(&mut self) -> Option<Self::Item> {
        let mut meta = Meta::default();

        // This temporary span is only used if there's an error reading the comment
        meta.span = self.span_start();

        loop {
            while self.peek().map(char::is_whitespace).unwrap_or(false) {
                self.next()?;
            }

            match self.peek() {
                Some('#') => {
                    let end = self.input.find('\n')
                        .map(|i| i + 1)
                        .unwrap_or(self.input.len());

                    meta.comments.push(self.input[1..end].to_string());
                    self.input = &self.input[end..];

                    self.row += 1;
                    self.col = 0;
                },
                Some('/') => {
                    if self.input[1..].chars().next() != Some('*') {
                        break;
                    }

                    let end = match self.input.find("*/") {
                        Some(end) => end,
                        None => return self.span_err(meta, TokenizeError::UnclosedComment)
                    };

                    let comment = self.input[2..end].to_string();

                    let target = &self.input[end+2..];
                    while self.input.as_ptr() < target.as_ptr() {
                        self.next()?;
                    }

                    meta.comments.push(comment);
                },
                _ => break
            }
        }

        // Check if it's a path
        let mut lookahead = self.input.chars().skip_while(|c| match c {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '_' | '.' | '+' | '-' => true,
            _ => false
        });
        let kind = match (lookahead.next(), lookahead.next()) {
            (Some(':'), Some(c)) if !c.is_whitespace() => Some(IdentType::Uri),
            (Some('/'), Some(c)) if !c.is_whitespace() => Some(IdentType::Path),
            _ => None
        };

        meta.span = self.span_start();
        let c = self.next()?;

        if c == '~' || kind == Some(IdentType::Path) {
            let (anchor, prefix) = match c {
                '~' => if self.next() != Some('/') {
                    return self.span_err(meta, TokenizeError::UndefinedToken);
                } else {
                    (Anchor::Home, None)
                },
                '/' => (Anchor::Absolute, Some('/')),
                c => (Anchor::Relative, Some(c))
            };
            let ident = self.next_ident(prefix, is_valid_path_char);
            if ident.ends_with('/') {
                return self.span_err(meta, TokenizeError::TrailingSlash);
            }
            return self.span_end(meta, Token::Value(Value::Path(anchor, ident)));
        }

        match c {
            '{' => self.span_end(meta, Token::CurlyBOpen),
            '}' => self.span_end(meta, Token::CurlyBClose),
            '=' => self.span_end(meta, Token::Equal),
            ';' => self.span_end(meta, Token::Semicolon),
            '[' => self.span_end(meta, Token::SquareBOpen),
            ']' => self.span_end(meta, Token::SquareBClose),
            '(' => self.span_end(meta, Token::ParenOpen),
            ')' => self.span_end(meta, Token::ParenClose),
            '+' if self.peek() == Some('+') => { self.next()?; self.span_end(meta, Token::Concat) },
            '+' => self.span_end(meta, Token::Add),
            '-' => self.span_end(meta, Token::Sub),
            '*' => self.span_end(meta, Token::Mul),
            '/' => self.span_end(meta, Token::Div),
            '.' => self.span_end(meta, Token::Dot),
            '<' => {
                let ident = self.next_ident(None, is_valid_path_char);
                if self.next() != Some('>') {
                    return self.span_err(meta, TokenizeError::UndefinedToken);
                }
                self.span_end(meta, Token::Value(Value::Path(Anchor::Store, ident)))
            },
            'a'..='z' | 'A'..='Z' => {
                let kind = kind.unwrap_or(IdentType::Ident);
                assert_ne!(kind, IdentType::Path, "paths are checked earlier");
                let ident = self.next_ident(Some(c), |c| match c {
                    'a'..='z' | 'A'..='Z' | '0'..='9' | '_' => true,
                    ':' | '?' | '@' | '&' | '=' | '$' | ',' | '!'
                        | '~' | '*' | '\'' | '%' => kind == IdentType::Uri,
                    c => kind == IdentType::Uri && is_valid_path_char(c),
                });
                if kind == IdentType::Ident {
                    self.span_end(meta, match &*ident {
                        "let" => Token::Let,
                        "in" => Token::In,
                        "with" => Token::With,
                        "import" => Token::Import,
                        "rec" => Token::Rec,
                        _ => Token::Ident(ident),
                    })
                } else {
                    self.span_end(meta, match kind {
                        IdentType::Ident => Token::Ident(ident),
                        IdentType::Path => Token::Value(Value::Path(Anchor::Relative, ident)),
                        IdentType::Uri => Token::Value(Value::Path(Anchor::Uri, ident)),
                    })
                }
            },
            '"' => self.next_string(meta, false),
            '\'' if self.peek() == Some('\'') => {
                self.next()?;
                self.next_string(meta, true)
            },
            '0'..='9' => {
                // Could use built-in rust parse function here, however that
                // requires collecting stuff to a string first, which is very
                // expensive.

                // TODO: Multiple radixes?
                const RADIX: u32 = 10;

                // We already know it's a digit
                let mut num = c.to_digit(RADIX).unwrap() as i64;

                while let Some(digit) = self.peek().and_then(|c| c.to_digit(RADIX)) {
                    self.next();
                    num = match num.checked_mul(RADIX as i64).and_then(|num| num.checked_add(digit as i64)) {
                        Some(num) => num,
                        None => return self.span_err(meta, TokenizeError::IntegerOverflow)
                    };
                }

                if self.peek() == Some('.') {
                    self.next();

                    let mut i = 1;
                    let mut num = num as f64;

                    while let Some(digit) = self.peek().and_then(|c| c.to_digit(RADIX)) {
                        self.next();
                        i *= RADIX;
                        num += digit as f64 / i as f64;
                    }

                    if i == 1 {
                        return self.span_err(meta, TokenizeError::TrailingDecimal)
                    }

                    self.span_end(meta, Token::Value(Value::Float(num)))
                } else {
                    self.span_end(meta, Token::Value(Value::Integer(num)))
                }
            },
            _ => self.span_err(meta, TokenizeError::UndefinedToken)
        }
    }
}

pub fn tokenize<'a>(input: &'a str) -> impl Iterator<Item = Item> + 'a {
    Tokenizer::new(input)
}

#[cfg(test)]
mod tests {
    use crate::value::{Anchor, Value};
    use super::{Interpol, Meta, Span, Token, TokenizeError};

    fn tokenize(input: &str) -> Result<Vec<Token>, TokenizeError> {
        super::tokenize(input)
            .map(|result| result
                .map(|(_, token)| token)
                .map_err(|(_, err)| err))
            .collect()
    }
    fn tokenize_span(input: &str) -> Result<Vec<(Meta, Token)>, (Span, TokenizeError)> {
        super::tokenize(input).collect()
    }

    #[test]
    fn basic_int_set() {
        assert_eq!(
            tokenize("{ int = 42; }"),
            Ok(vec![Token::CurlyBOpen, Token::Ident("int".into()), Token::Equal,
            Token::Value(42.into()), Token::Semicolon, Token::CurlyBClose])
        );
    }
    #[test]
    fn basic_float_set() {
        assert_eq!(
            tokenize("{ float = 1.234; }"),
            Ok(vec![Token::CurlyBOpen, Token::Ident("float".into()), Token::Equal,
            Token::Value(1.234.into()), Token::Semicolon, Token::CurlyBClose])
        );
    }
    #[test]
    fn basic_string_set() {
        assert_eq!(
            tokenize(r#"{ string = "Hello \"World\""; }"#),
            Ok(vec![Token::CurlyBOpen, Token::Ident("string".into()), Token::Equal,
            Token::Value("Hello \"World\"".into()), Token::Semicolon, Token::CurlyBClose])
        );
    }
    #[test]
    fn meta() {
        assert_eq!(
            tokenize_span("{\n    int /* hi */ = 1; # testing comments!\n}"),
            Ok(vec![
                (meta! { start: (0,  0), end: (0,  1) }, Token::CurlyBOpen),
                (meta! { start: (1,  4), end: (1,  7) }, Token::Ident("int".to_string())),
                (
                    Meta {
                        comments: vec![" hi ".into()],
                        span: Span { start: (1, 17), end: Some((1, 18)) },
                    },
                    Token::Equal
                ),
                (meta! { start: (1, 19), end: (1, 20) }, Token::Value(1.into())),
                (meta! { start: (1, 20), end: (1, 21) }, Token::Semicolon),
                (
                    Meta {
                        comments: vec![" testing comments!\n".into()],
                        span: Span { start: (2,  0), end: Some((2,  1)) }
                    },
                    Token::CurlyBClose
                )
            ])
        );
        assert_eq!(
            tokenize_span("{\n    overflow = 9999999999999999999999999999"),
            Err((Span { start: (1, 15), end: None }, TokenizeError::IntegerOverflow))
        );
    }
    #[test]
    fn multiline() {
        assert_eq!(
            tokenize(r#"{
    multiline = ''
        This is a
        multiline
        string :D
        \'\'\'\'\
    '';
}"#),
            Ok(vec![
                Token::CurlyBOpen,
                    Token::Ident("multiline".into()), Token::Equal,
                    Token::Value(r#"This is a
multiline
string :D
\'\'\'\'\
"#.into()),
                Token::Semicolon, Token::CurlyBClose
            ])
        );
    }
    #[test]
    fn interpolation() {
        assert_eq!(
            tokenize_span(r#" "Hello, ${ { world = "World"; }.world }!" "#),
            Ok(vec![(
                meta! { start: (0, 1), end: (0, 42) },
                Token::Interpol(vec![
                    Interpol::Literal("Hello, ".into()),
                    Interpol::Tokens(vec![
                        (meta! { start: (0, 12), end: (0, 13) }, Token::CurlyBOpen),
                        (meta! { start: (0, 14), end: (0, 19) }, Token::Ident("world".into())),
                        (meta! { start: (0, 20), end: (0, 21) }, Token::Equal),
                        (meta! { start: (0, 22), end: (0, 29) }, Token::Value("World".into())),
                        (meta! { start: (0, 29), end: (0, 30) }, Token::Semicolon),
                        (meta! { start: (0, 31), end: (0, 32) }, Token::CurlyBClose),
                        (meta! { start: (0, 32), end: (0, 33) }, Token::Dot),
                        (meta! { start: (0, 33), end: (0, 38) }, Token::Ident("world".into()))
                    ]),
                    Interpol::Literal("!".into())
                ])
            )])
        );
    }
    #[test]
    fn math() {
        assert_eq!(
            tokenize("1 + 2 * 3"),
            Ok(vec![Token::Value(1.into()), Token::Add, Token::Value(2.into()), Token::Mul, Token::Value(3.into())])
        );
        assert_eq!(
            tokenize("5 * -(3 - 2)"),
            Ok(vec![
                Token::Value(5.into()), Token::Mul,
                Token::Sub, Token::ParenOpen,
                    Token::Value(3.into()), Token::Sub, Token::Value(2.into()),
                Token::ParenClose
            ])
        );
        assert_eq!(
            tokenize("a/ 3"), // <- could get mistaken for a path
            Ok(vec![Token::Ident("a".into()), Token::Div, Token::Value(3.into())])
        );
    }
    #[test]
    fn let_in() {
        assert_eq!(
            tokenize("let a = 3; in a"),
            Ok(vec![
                Token::Let,
                    Token::Ident("a".into()), Token::Equal, Token::Value(3.into()), Token::Semicolon,
                Token::In,
                    Token::Ident("a".into())
            ])
        );
    }
    #[test]
    fn with() {
        assert_eq!(
            tokenize("with namespace; expr"),
            Ok(vec![
                Token::With, Token::Ident("namespace".into()), Token::Semicolon,
                Token::Ident("expr".into())
            ])
        );
    }
    #[test]
    fn paths() {
        fn path(anchor: Anchor, path: &str) -> Result<Vec<Token>, TokenizeError> {
            Ok(vec![Token::Value(Value::Path(anchor, path.into()))])
        }
        assert_eq!(tokenize("/hello/world"), path(Anchor::Absolute, "/hello/world"));
        assert_eq!(tokenize("hello/world"), path(Anchor::Relative, "hello/world"));
        assert_eq!(tokenize("a+3/5+b"), path(Anchor::Relative, "a+3/5+b"));
        assert_eq!(tokenize("1-2/3"), path(Anchor::Relative, "1-2/3"));
        assert_eq!(tokenize("./hello/world"), path(Anchor::Relative, "./hello/world"));
        assert_eq!(tokenize("~/hello/world"), path(Anchor::Home, "hello/world"));
        assert_eq!(tokenize("<hello/world>"), path(Anchor::Store, "hello/world"));
        assert_eq!(
            tokenize("https://google.com/?q=Hello+World"),
            path(Anchor::Uri, "https://google.com/?q=Hello+World")
        );
    }
    #[test]
    fn import() {
        assert_eq!(
            tokenize("import <nixpkgs>"),
            Ok(vec![
                Token::Import,
                Token::Value(Value::Path(Anchor::Store, "nixpkgs".into()))
            ])
        );
    }
    #[test]
    fn list() {
        assert_eq!(
            tokenize(r#"[a 2 3 "lol"]"#),
            Ok(vec![
               Token::SquareBOpen,
               Token::Ident("a".into()), Token::Value(2.into()), Token::Value(3.into()),
               Token::Value("lol".into()),
               Token::SquareBClose
            ])
        );
        assert_eq!(
            tokenize(r#"[1] ++ [2] ++ [3]"#),
            Ok(vec![
               Token::SquareBOpen, Token::Value(1.into()), Token::SquareBClose, Token::Concat,
               Token::SquareBOpen, Token::Value(2.into()), Token::SquareBClose, Token::Concat,
               Token::SquareBOpen, Token::Value(3.into()), Token::SquareBClose
            ])
        );
    }
}