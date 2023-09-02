use std::mem;

pub fn remove_indices<T: Default>(mut v: Vec<T>, to_be_deleted: &[usize]) -> Vec<T> {
    let mut target_idx = 0;
    for src_idx in 0..v.len() {
        if !to_be_deleted.contains(&src_idx) {
            if src_idx != target_idx {
                v[target_idx] = mem::take(&mut v[src_idx]);
            }
            target_idx += 1;
        }
    }
    v.truncate(target_idx);
    v
}
