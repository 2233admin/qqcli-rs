# Decoupling Compliance Report — qqcli-rs

> Generated 2026-06-07. Source of truth for the 5 件事 解耦 refactor.
> 后续 task 完成后, 把这一节里的 ❌ 改成 ✅, commit 里 reference 本文件.

## 5 件事解耦目标

| ID | 一句话 | 验收 (绿 = 通过) |
|---|---|---|
| **A** | Forward 嵌套递归 (depth 5), 不暴露 `Unknown{reason: "nested forward"}` 给上层 | ❌ → #3-new |
| **B** | db.rs 跟 normalize.rs 解耦, db.rs 不依赖 normalize 模块 | ❌ → #4-new |
| **C** | bundle 走 Segment 列表, 不 regex 字符串 | ❌ → #5-new |
| **D** | export 走 `content_inline` (从 segments 派生), 不直接读 `m.content` | ❌ → #6-new |
| **E** | search segment 级索引 + DuckDB 真 FTS | ❌ → #7-new |

## 当前泄露点 (37 处, 5 类)

### A. `Segment::Unknown{reason: "nested forward ..."}` 占位符 (6 处)
- `src/normalize.rs:546-549` — pathway 2 `multiForwardMsgElement` nested forward 占位
- `src/normalize.rs:1095-1098` — OneBot forward nested 占位
- `src/normalize.rs:1234, 1452, 1466` — 3 个相关 test 断言当前是 Unknown (绿了反而说明"还耦合着", 这些 test 改造时**必须**改或删)
- **根因**: `expand_nested_forwards` 还没实现
- **修复**: Task #3-new 加 `expand_nested_forwards(&mut MessageWithSegments, depth: usize)`, depth=5 默认

### B. db.rs ↔ normalize.rs 耦合 (5 处 + 模块依赖)
- `src/db.rs:194, 613, 659` — `extract_text(&content_raw)` 直接调 db 自己的 text 提取
- `src/db.rs:1000, 1053, 1079` — `extract_text` 跟 `extract_text_from_blob` 跟 `extract_text_from_blob_scanned` 三个函数 (blob → text 旧路径)
- `src/db_index.rs:81, 128` — DuckDB import_all 调 `db::extract_text` 跟 `db::fmt_ts` (会耦合)
- **根因**: db.rs 自己实现了 text 提取, 跟 normalize.rs 的 `normalize_blob_to_segments` 是**两套并行实现**, 信息没回流
- **修复**: Task #4-new
  1. db.rs::extract_text 系列函数标 `#[deprecated]` 跟 `// moved to normalize::extract_text`
  2. `commands.rs` 跟 `db_index.rs` 改成调 `crate::normalize::normalize_blob_to_segments` 然后读 `mws.content_inline`
  3. `db.rs` 只保留 `extract_text` 作为 fallback (DB 字段是 TEXT 不是 BLOB 的场景), 加注释说明"仅当 blob 是 text 编码, 真 message 请走 normalize"

### C. bundle 字符串 regex (5 处)
- `src/commands.rs:260` — `for url in extract_image_urls(&m.content)` ← **直接读 m.content**
- `src/commands.rs:318-336` — `extract_image_urls` 函数 19 行
- `src/commands.rs:322` — `if line.contains("multimedia.nt.qq.com.cn")` ← **唯一 CDN**
- `src/commands.rs:330` — `format!("https://multimedia.nt.qq.com.cn{}", params)` ← **hardcode 域名**
- `src/commands.rs:260` — `m.content` 直接读 ← 同时是 D 类
- **根因**: 早于 Segment 架构 (Segment 是 #1 才加的), 旧实现没改
- **修复**: Task #5-new
  1. 删 `extract_image_urls` 函数 (19 行)
  2. bundle 改成 `for seg in &m.segments { match seg { Segment::Image{url, fileid, local_path, ..} => ..., Segment::Record{..} => ..., Segment::File{..} => ..., _ => skip } }`
  3. 路径解析走 `media::resolve_local_path` (Task #4 的产物) + `media::candidate_cdn_urls` 多 CDN 列表

### D. export / output / commands 直接读 m.content (6 处)
- `src/commands.rs:137` — `search` fallback 路径里 `if r.content.len() > 100 { format!("{}...", &r.content[..100]) }` — 跟 Task #7 search 一起改
- `src/commands.rs:207` — export `txt` 格式拼 `format!("[{}] {}: {}\n", m.time_str, m.sender_name, m.content)`
- `src/commands.rs:223` — export `markdown` 格式 `format!("**{}** [{}]: {}\n", m.time_str, sender, m.content)`
- `src/output.rs:54` — `YAML writer` `println!("{} {} {}: {}", ..., m.content)`
- `src/db_index.rs:81, 128` — DuckDB import_all 写 `let content = db::extract_text(&content_raw)` 拿 inline
- **根因**: 同 C, 早于 Segment 架构
- **修复**: Task #6-new
  1. `m.content` 字段**保留** (向后兼容, YamlWriter/JSON consumer 都靠它)
  2. **但** 写入 m.content 的地方必须从 `mws.content_inline` (segments 派生) 来, 不是从 blob 抽 text
  3. export markdown 加 segment 注释: `[图片]` / `[文件:foo.pdf]` / `[转发:3条]` inline 标注, 让 m.content 不再是裸 text
  4. db_index import_all 改成 `let mws = normalize_blob_to_segments(&raw); insert(..., mws.content_inline, ..., mws.segments_json)`

### E. search 没 segment 级索引 + 不是真 FTS
- `src/db_index.rs:194` — `let pattern = format!("%{}%", query);` ← **LIKE 模糊, 不是 FTS**
- `src/db_index.rs:181-191` — SQL 模板只查 `content` 一个 TEXT 列
- **根因**: Task #1 计划里写的 "DuckDB FTS" 实际是 LIKE, 跟 #7 真 FTS 差
- **修复**: Task #7-new
  1. DuckDB schema 加 `segments TEXT` 字段 (JSON 序列化的 Vec<Segment>)
  2. 跑 `INSTALL fts; LOAD fts; CREATE INDEX idx_content_fts USING FTS ON messages(content)` 一次性 migration
  3. search SQL 改 `WHERE fts_main_content.match_bm25(content, ?)` (DuckDB FTS 标准)
  4. CLI 接受前缀 `pic:<kw>` / `file:<kw>` / `forward:` 走 segments JSON 过滤

## 验证矩阵 (绿 = 通过)

```bash
# A — Forward 递归
cargo test forward_two_level_recursion -- --exact
# B — db 解耦
grep -rn "use crate::normalize" src/db.rs   # 0 match 才对
grep -rn "db::extract_text" src/  # 0 match 才对 (除了 #[deprecated] 注释)
# C — bundle 走 segment
grep -n "extract_image_urls" src/commands.rs  # 0 match 才对
grep -n "multimedia.nt.qq.com.cn" src/commands.rs  # 0 match 才对 (移到 media.rs)
# D — export 走 content_inline
grep -n "m\.content" src/commands.rs | grep -v "mws.content_inline"  # 0 match 才对
# E — 真 FTS
grep -n "fts_main_content" src/db_index.rs  # >=1 match
grep -n "format!(\"%{}%\"" src/db_index.rs  # 0 match 才对
```

## 进度追踪 (后续 task 完成时勾)

- [x] C: bundle 走 Segment — **9970f59** 修 (删除 extract_image_urls 19 行 regex, 改走 m.segments, 加 backfill_segment_urls 3 层 url 还原)
- [ ] A: Forward 嵌套递归 — #16
- [ ] B: db ↔ normalize 解耦 — #17
- [ ] D: export 走 content_inline — #19
- [ ] E: search segment FTS — #20

## QA Deferred (2026-06-07 跑出, 已修)

| Bug | 状态 | 修法 | 关联 |
|---|---|---|---|
| BUG-001 export --format 静默 | ✅ 9970f59 | anyhow::bail 显式报错 | — |
| BUG-002 bundle 拿不到图 | ⚠️ 9970f59 80% | 走 Segment 列表, 拼 NT CDN url, 缺 rkey | NapCat 集成后续 |
| BUG-003 uid 全显示 `uid_xxx` | ⚠️ c34d960 框架就位 | 新加 `uid_resolve.rs`, DB 里 `[48912]` 全空物理上解不出真名, 需 `qq sync` 走 NapCat 拉 | 后续 NapCat 集成 |
| BUG-004 旧 groupCode 静默 | ✅ c34d960 | members 0 成员时 eprintln 提示, 引导用 `qq sessions` 找新 ID | — |
| BUG-005 search 输出 sender_name | ⚠️ c34d960 框架就位 | 同 BUG-003, DB 无真名 | 后续 NapCat 集成 |

健康分: 72 → 92 (5 个 bug 全识别, 2 修完整, 3 修框架留 NapCat 集成)

QA 报告: `.gstack/qa-reports/qa-report-qqcli-rs-2026-06-07.md`

## 关联 sub-file

- `project_qqcli_rs_decoupling.md` (memory) — 决策历史
- `qqcli-rs/src/segment.rs` — 9-variant Segment
- `qqcli-rs/src/normalize.rs` — 5-pathway normalizer
- `qqcli-rs/src/db.rs` — SQLite reader (待解耦)
- `qqcli-rs/src/db_index.rs` — DuckDB importer (待加 FTS)
- `qqcli-rs/src/commands.rs` — CLI (待走 segment 列表)
