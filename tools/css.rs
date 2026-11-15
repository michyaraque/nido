// CSS minifier shared by the build scripts of Nido and its plugins (`include!`d, no crate): the
// stylesheets stay readable in the repository and ship compact. It drops comments, collapses
// whitespace, removes it around `{ } ; ,` and after `:`, and drops the last `;` of a block.
// Strings keep their spaces, and so do `( )` except next to commas (`calc(100% + 8px)` needs them).

pub fn minify_css(css: &str) -> String {
    let b: Vec<char> = css.chars().collect();
    let mut out = String::with_capacity(css.len());
    let mut space = false;
    let mut paren = 0usize;
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        if c == '/' && b.get(i + 1) == Some(&'*') {
            i += 2;
            while i < b.len() && !(b[i] == '*' && b.get(i + 1) == Some(&'/')) {
                i += 1;
            }
            i += 2;
            space = true;
            continue;
        }
        if c.is_whitespace() {
            space = true;
            i += 1;
            continue;
        }
        if space {
            let tight = |ch: char| "{};,".contains(ch);
            if let Some(p) = out.chars().last() {
                if !(tight(p) || p == ':' || (paren == 0 && tight(c)) || c == ',') {
                    out.push(' ');
                }
            }
            space = false;
        }
        match c {
            '"' | '\'' => {
                out.push(c);
                i += 1;
                while i < b.len() && b[i] != c {
                    if b[i] == '\\' && i + 1 < b.len() {
                        out.push(b[i]);
                        i += 1;
                    }
                    out.push(b[i]);
                    i += 1;
                }
                if i < b.len() {
                    out.push(c);
                }
                i += 1;
                continue;
            }
            '(' => paren += 1,
            ')' => paren = paren.saturating_sub(1),
            '}' if paren == 0 && out.ends_with(';') => {
                out.pop();
            }
            _ => {}
        }
        out.push(c);
        i += 1;
    }
    out
}
