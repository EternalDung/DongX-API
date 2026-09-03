//! 文本分块：把长文本按段落切分为若干块，块内不超过 `max_chars`，
//! 相邻块保留 `overlap` 字符重叠以缓解边界信息丢失。

/// 按段落（`\n\n`）聚合切分；单段超过 `max_chars` 时也按段落边界断开，
/// 不强行在段内硬切。返回非空（至少含原文）。
pub fn chunk_text(text: &str, max_chars: usize, overlap: usize) -> Vec<String> {
    let max_chars = max_chars.max(100);
    let overlap = overlap.min(max_chars / 2);

    let paras: Vec<String> = text
        .split("\n\n")
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    let mut chunks: Vec<String> = Vec::new();
    let mut current = String::new();
    for p in paras {
        if current.is_empty() {
            current = p;
        } else if current.chars().count() + p.chars().count() + 2 <= max_chars {
            current.push_str("\n\n");
            current.push_str(&p);
        } else {
            let tail: String = current
                .chars()
                .skip(current.chars().count().saturating_sub(overlap))
                .collect();
            chunks.push(std::mem::take(&mut current));
            current = if overlap > 0 {
                format!("{}\n\n{}", tail, p)
            } else {
                p
            };
        }
    }
    if !current.is_empty() {
        chunks.push(current);
    }
    if chunks.is_empty() {
        chunks.push(text.to_string());
    }
    chunks
}
