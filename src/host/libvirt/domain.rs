use askama::Template;

#[derive(Template)]
#[template(path = "domain.xml")]
pub struct DomainTemplate {
    pub name: String,
}