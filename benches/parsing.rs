use criterion::{black_box, criterion_group, criterion_main, Criterion};
use iptv_proxy::playlist::parse_playlist;
use iptv_proxy::rewriter::rewrite_hls;

fn bench_parse_playlist(c: &mut Criterion) {
    let content = include_str!("../playlist.m3u8.example");
    c.bench_function("parse_playlist", |b| {
        b.iter(|| parse_playlist(black_box(content)))
    });
}

fn bench_rewrite_hls(c: &mut Criterion) {
    let manifest = r#"#EXTM3U
#EXT-X-VERSION:3
#EXT-X-TARGETDURATION:6
#EXT-X-MEDIA-SEQUENCE:12345
seg001.ts
seg002.ts
seg003.ts
seg004.ts
seg005.ts
"#;
    c.bench_function("rewrite_hls", |b| {
        b.iter(|| {
            rewrite_hls(
                black_box(manifest),
                "https://cdn.example.com/stream/",
                "http://localhost:8888",
                "ctx_placeholder",
            )
        })
    });
}

criterion_group!(benches, bench_parse_playlist, bench_rewrite_hls);
criterion_main!(benches);
