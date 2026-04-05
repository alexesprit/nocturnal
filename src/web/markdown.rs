use pulldown_cmark::{Options, Parser, html};

pub fn render(source: &str) -> String {
    let parser = Parser::new_ext(source, Options::ENABLE_TABLES | Options::ENABLE_TASKLISTS);
    let mut html_output = String::new();
    html::push_html(&mut html_output, parser);
    ammonia::Builder::new()
        .add_tags(&["input"])
        .add_tag_attributes("input", &["type", "checked", "disabled"])
        .clean(&html_output)
        .to_string()
}
