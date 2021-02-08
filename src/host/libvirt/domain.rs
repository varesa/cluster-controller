use askama::Template;

#[derive(Template)]
#[template(path = "domain.xml")]
pub struct DomainTemplate {
    pub name: String,
    pub uuid: String,
    pub cpus: u8,
    pub memory: u32,
    pub memory_unit: String,
}