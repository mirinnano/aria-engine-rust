# Native と Web の一致を確かめる入力記録

`vertical-slice-inputs.json` は、読者の入力を時刻つきで並べた記録である。
`crates/aria-cli/tests/cross_runtime_determinism.rs` がこの記録を Native と Web の両方で再生し、毎フレームの出力と最後の保存データのハッシュが一致することを確かめる。
ある振る舞いを Native と Web で同じにすると約束するときは、その振る舞いを通る入力をこの記録に足す。
