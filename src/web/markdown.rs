use pulldown_cmark::{Options, Parser, html};

pub fn render(source: &str) -> String {
    let parser = Parser::new_ext(source, Options::ENABLE_TABLES);
    let mut html_output = String::new();
    html::push_html(&mut html_output, parser);
    ammonia::clean(&html_output)
}
