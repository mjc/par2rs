#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#define PARPAR_INCLUDE_BASIC_OPS
#include "../../par2cmdline-turbo/parpar/gf16/gf16_xor_avx2.c"

static void fill_pattern(uint8_t* dst, size_t len) {
    for(size_t i = 0; i < len; i++) {
        dst[i] = (uint8_t)((i * 37u + 11u) & 0xffu);
    }
}

int main(int argc, char** argv) {
    const char* output_path = argc > 1 ? argv[1] : NULL;
    size_t src_len = argc > 2 ? strtoull(argv[2], NULL, 0) : 1024 * 1024;
    size_t slice_len = argc > 3 ? strtoull(argv[3], NULL, 0) : 1024 * 1024;
    size_t chunk_len = argc > 4 ? strtoull(argv[4], NULL, 0) : 128 * 1024;
    unsigned input_pack_size = argc > 5 ? (unsigned)strtoul(argv[5], NULL, 0) : 12;
    unsigned input_num = argc > 6 ? (unsigned)strtoul(argv[6], NULL, 0) : 5;

    size_t segment_count = (slice_len + chunk_len - 1) / chunk_len;
    size_t dst_len = segment_count * input_pack_size * chunk_len;

    uint8_t* src = (uint8_t*)malloc(src_len);
    uint8_t* dst = (uint8_t*)calloc(dst_len, 1);
    if(!src || !dst) {
        fprintf(stderr, "allocation failed\n");
        return 1;
    }

    fill_pattern(src, src_len);
    gf16_xor_prepare_packed_avx2(dst, src, src_len, slice_len, input_pack_size, input_num, chunk_len);

    FILE* fp = output_path ? fopen(output_path, "wb") : stdout;
    if(!fp) {
        perror("fopen");
        return 1;
    }
    fwrite(dst, 1, dst_len, fp);
    if(output_path)
        fclose(fp);

    free(dst);
    free(src);
    return 0;
}
