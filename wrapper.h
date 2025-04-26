#include "external/HDiffPatch/hpatch_dir_listener.h"
#include "external/HDiffPatch/compress_parallel.h"
#include "external/HDiffPatch/_dir_ignore.h"
#include "external/HDiffPatch/_atosize.h"

// DirDiffPatch
#include "external/HDiffPatch/dirDiffPatch/dir_diff/dir_diff.h"
#include "external/HDiffPatch/dirDiffPatch/dir_diff/dir_manifest.h"
#include "external/HDiffPatch/dirDiffPatch/dir_diff/dir_diff_tools.h"
#include "external/HDiffPatch/dirDiffPatch/dir_diff/file_for_dirDiff.h"

#include "external/HDiffPatch/dirDiffPatch/dir_patch/dir_patch.h"
//#include "external/HDiffPatch/dirDiffPatch/dir_patch/dir_patch_private.h"
#include "external/HDiffPatch/dirDiffPatch/dir_patch/dir_patch_tools.h"
#include "external/HDiffPatch/dirDiffPatch/dir_patch/dir_patch_types.h"
#include "external/HDiffPatch/dirDiffPatch/dir_patch/new_dir_output.h"
#include "external/HDiffPatch/dirDiffPatch/dir_patch/new_stream.h"
#include "external/HDiffPatch/dirDiffPatch/dir_patch/ref_stream.h"
#include "external/HDiffPatch/dirDiffPatch/dir_patch/res_handle_limit.h"

// libHdiffPatch
#include "external/HDiffPatch/libHDiffPatch/HDiff/diff.h"
#include "external/HDiffPatch/libHDiffPatch/HDiff/diff_types.h"
#include "external/HDiffPatch/libHDiffPatch/HDiff/diff_for_hpatch_lite.h"
#include "external/HDiffPatch/libHDiffPatch/HDiff/match_block.h"

#include "external/HDiffPatch/libHDiffPatch/HPatch/checksum_plugin.h"
#include "external/HDiffPatch/libHDiffPatch/HPatch/patch.h"
//#include "external/HDiffPatch/libHDiffPatch/HPatch/patch_private.h"
#include "external/HDiffPatch/libHDiffPatch/HPatch/patch_types.h"

#include "external/HDiffPatch/libHDiffPatch/HPatchLite/hpatch_lite.h"
#include "external/HDiffPatch/libHDiffPatch/HPatchLite/hpatch_lite_types.h"
#include "external/HDiffPatch/libHDiffPatch/HPatchLite/hpatch_lite_input_cache.h"

/*#include "external/HDiffPatch/libHDiffPatch/HDiff/private_diff/bytes_rle.h"
#include "external/HDiffPatch/libHDiffPatch/HDiff/private_diff/compress_detect.h"
#include "external/HDiffPatch/libHDiffPatch/HDiff/private_diff/mem_buf.h"
#include "external/HDiffPatch/libHDiffPatch/HDiff/private_diff/pack_uint.h"
#include "external/HDiffPatch/libHDiffPatch/HDiff/private_diff/qsort_parallel.h"
#include "external/HDiffPatch/libHDiffPatch/HDiff/private_diff/suffix_string.h"*/

/*#include "external/HDiffPatch/libHDiffPatch/HDiff/private_diff/limit_mem_diff/adler_roll.h"
#include "external/HDiffPatch/libHDiffPatch/HDiff/private_diff/limit_mem_diff/bloom_filter.h"
#include "external/HDiffPatch/libHDiffPatch/HDiff/private_diff/limit_mem_diff/covers.h"
#include "external/HDiffPatch/libHDiffPatch/HDiff/private_diff/limit_mem_diff/digest_matcher.h"
#include "external/HDiffPatch/libHDiffPatch/HDiff/private_diff/limit_mem_diff/stream_serialize.h"*/

/*#include "external/HDiffPatch/libHDiffPatch/HDiff/private_diff/libdivsufsort/config.h"
#include "external/HDiffPatch/libHDiffPatch/HDiff/private_diff/libdivsufsort/divsufsort.h"
#include "external/HDiffPatch/libHDiffPatch/HDiff/private_diff/libdivsufsort/divsufsort64.h"*/
//#include "external/HDiffPatch/libHDiffPatch/HDiff/private_diff/libdivsufsort/divsufsort_private.h"

