mod utils;
pub mod patchers;

#[cfg(test)]
mod tests {
    use crate::patchers::{HDiff, KrDiff};

    #[test]
    fn apply_krdiff_patch() {
        let src = String::from("/games/kuro/wuwa_global/c7s90wschv2dj8d9ilfdzifl");
        let krdiff = String::from("/games/kuro/wuwa_global/c7s90wschv2dj8d9ilfdzifl/patching/staging/2.6.2_2.7.0_group_30_1758874286190.krpdiff");
        let dst = String::from("/games/kuro/wuwa_global/c7s90wschv2dj8d9ilfdzifl/patching/staging");

        let mut krd = KrDiff::new(src, krdiff, Some(dst));
        let status = krd.apply();
        if status { println!("krdiff applied successfully"); } else { println!("krdiff apply failed"); }
    }

    #[test]
    fn apply_hdiff_patch() {
        let src = String::from("<path to game dir>");
        let hdiff = String::from("./2.5.1_2.6.0.hdiff");
        let out = String::from("./2.5.1_2.6.1.blk");

        let mut hd = HDiff::new(src, hdiff, out);
        let status = hd.apply();
        if status { println!("hdiff applied successfully"); } else { println!("hdiff apply failed"); }
    }
}
