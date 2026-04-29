#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>

#include "../../par2cmdline-turbo/parpar/gf16/gf16_xor_avx2.c"

int main(int argc, char** argv) {
    uint16_t coefficient = (uint16_t)strtoul(argc > 1 ? argv[1] : "0xc814", NULL, 0);
    int prefetch = argc > 2 ? atoi(argv[2]) : 0;
    const char* output_path = argc > 3 ? argv[3] : NULL;

    struct gf16_xor_scratch* scratch =
        (struct gf16_xor_scratch*)gf16_xor_jit_init_avx2(0x1100b, GF16_XOR_JIT_STRAT_NONE);
    jit_wx_pair* jit = (jit_wx_pair*)gf16_xor_jit_init_mut_avx2();
    if(!scratch || !jit) {
        fprintf(stderr, "failed to initialize turbo xor-jit scratch\n");
        return 1;
    }

    uint8_t* body_start = (uint8_t*)jit->w + scratch->codeStart;
    uint8_t* end =
        xor_write_jit_avx(scratch, body_start, coefficient, XORDEP_JIT_MODE_MULADD, prefetch ? _MM_HINT_T1 : 0);
    write32(end, (int32_t)((uint8_t*)jit->w - end - 4));
    end[4] = 0xC3;
    end += 5;

    size_t total_len = (size_t)(end - (uint8_t*)jit->w);
    size_t dynamic_len = (size_t)(end - body_start);
    fprintf(
        stderr,
        "turbo xor-jit coeff=%#06x prefetch=%d code_start=%u total_len=%zu dynamic_len=%zu\n",
        coefficient,
        prefetch,
        (unsigned)scratch->codeStart,
        total_len,
        dynamic_len
    );

    FILE* fp = output_path ? fopen(output_path, "wb") : stdout;
    if(!fp) {
        perror("fopen");
        return 1;
    }
    fwrite(jit->w, 1, total_len, fp);
    if(output_path)
        fclose(fp);

    jit_free(jit);
    ALIGN_FREE(scratch);
    return 0;
}
