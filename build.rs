fn main() {
    embed_resource::compile("assets/logo.rc", embed_resource::NONE)
        .manifest_required()
        .unwrap();
}
