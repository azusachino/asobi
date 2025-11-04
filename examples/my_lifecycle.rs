#![allow(unused)]

fn main() {}

#[derive(Debug)]
struct Url<'a> {
    protocol: &'a str,
    host: &'a str,
    path: &'a str,
    query: &'a str,
    fragment: &'a str,
}

#[test]
fn test() {
    let url_str = "https://rustcc.cn/article?id=019f9937#title".to_owned();
    let url = Url {
        protocol: &url_str[0..5],
        host: &url_str[8..17],
        path: &url_str[17..25],
        query: &url_str[26..37],
        fragment: &url_str[38..43],
    };
    println!("{:?}", url);
}
