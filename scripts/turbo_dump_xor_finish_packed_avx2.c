#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#define PARPAR_INCLUDE_BASIC_OPS
#include "../../par2cmdline-turbo/parpar/gf16/gf16_xor_avx2.c"

static void fill_pattern(uint8_t* dst, size_t len) {
    for(size_t i = 0; i < len; i++) {
        dst[i] = (uint8_t)((i * 29u + 7u) & 0xffu);
    }
}

int main(int argc, char** argv) {
    const char* output_path = argc > 1 ? argv[1] : NULL;
    size_t slice_len = argc > 2 ? strtoull(argv[2], NULL, 0) : 1024 * 1024;
    size_t chunk_len = argc > 3 ? strtoull(argv[3], NULL, 0) : 128 * 1024;
    unsigned num_outputs = argc > 4 ? (unsigned)strtoul(argv[4], NULL, 0) : 7;
    unsigned output_num = argc > 5 ? (unsigned)strtoul(argv[5], NULL, 0) : 3;

    size_t segment_count = (slice_len + chunk_len - 1) / chunk_len;
    size_t prepared_len = segment_count * num_outputs * chunk_len;
    uint8_t* prepared = (uint8_t*)calloc(prepared_len, 1);
    uint8_t* input = (uint8_t*)malloc(slice_len);
    uint8_t* output = (uint8_t*)malloc(slice_len);
    if(!prepared || !input || !output) {
        fprintf(stderr, "allocation failed\n");
        return 1;
    }

    fill_pattern(input, slice_len);
    gf16_xor_prepare_packed_avx2(prepared, input, slice_len, slice_len, num_outputs, output_num, chunk_len);

    gf16_xor_finish_packed_avx2(output, prepared, slice_len, num_outputs, output_num, chunk_len);

    FILE* fp = output_path ? fopen(output_path, "wb") : stdout;
    if(!fp) {
        perror("fopen");
        return 1;
    }
    fwrite(output, 1, slice_len, fp);
    if(output_path)
        fclose(fp);

    free(output);
    free(input);
    free(prepared);
    return 0;
}
