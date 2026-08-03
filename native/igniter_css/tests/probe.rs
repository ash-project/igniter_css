use igniter_css::ctx::ParseCtx;
#[test]
fn p() {
    for src in [
        "@import \"a.css\";",
        "@import url(\"/a.css\");",
        "@import url(/a.css);",
        "@import 'a.css' screen;",
        "@plugin \"../vendor/x\";",
        "@source \"../js\";",
    ] {
        println!("===== {src}");
        println!("{:#?}", ParseCtx::parse_default(src).syntax());
    }
}
