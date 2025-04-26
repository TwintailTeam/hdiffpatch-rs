#![allow(non_upper_case_globals)]
#![allow(non_camel_case_types)]
#![allow(non_snake_case)]

include!(concat!(env!("OUT_DIR"), "/bindings.rs"));

#[cfg(test)]
mod tests {
    use std::ffi::c_uchar;
    use crate::{patch};

    #[test]
    fn apply_patch() {
        let mut old_data: Vec<u8> = vec![/* source data */];
        let patch_data: Vec<u8> = vec![/* patch data */];

        let mut out_data: Vec<u8> = Vec::new();
        let mut out_size: usize = 0;
        
        unsafe {
            patch(old_data.as_mut_ptr(), old_data.as_mut_ptr(), patch_data.as_ptr(), patch_data.len() as *mut c_uchar, out_data.as_mut_ptr(), out_size as *const c_uchar);
        }
    }
}
