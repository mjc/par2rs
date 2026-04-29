#include <cstddef>
#include <cstdint>
#include <mutex>
#include <new>

using std::max_align_t;
using std::size_t;
using std::uint8_t;
using std::uint16_t;
using std::uint32_t;
using std::uint64_t;
using std::uintptr_t;

#include "par2cmdline-turbo/parpar/hasher/hasher_input.h"
#include "par2cmdline-turbo/parpar/hasher/hasher_md5crc.h"

namespace {
enum ParParHasherInputMethod : unsigned {
    PARPAR_HASHER_INPUT_SCALAR = 0,
    PARPAR_HASHER_INPUT_SIMD = 1,
    PARPAR_HASHER_INPUT_CRC = 2,
    PARPAR_HASHER_INPUT_SIMD_CRC = 3,
    PARPAR_HASHER_INPUT_BMI1 = 4,
    PARPAR_HASHER_INPUT_AVX512 = 5,
};

static void configure_md5single() {
    static std::once_flag configured;
    std::call_once(configured, [] {
        if (!set_hasherMD5CRC(MD5CRCMETH_BMI1)) {
            (void)set_hasherMD5CRC(MD5CRCMETH_SCALAR);
        }
    });
}

static IHasherInput* create_input(ParParHasherInputMethod method) {
    switch (method) {
        case PARPAR_HASHER_INPUT_SCALAR:
            return HasherInput_Scalar::create();
        case PARPAR_HASHER_INPUT_SIMD:
            return HasherInput_SSE::create();
        case PARPAR_HASHER_INPUT_CRC:
            return HasherInput_ClMulScalar::create();
        case PARPAR_HASHER_INPUT_SIMD_CRC:
            return HasherInput_ClMulSSE::create();
        case PARPAR_HASHER_INPUT_BMI1:
            return HasherInput_BMI1::create();
        case PARPAR_HASHER_INPUT_AVX512:
            return HasherInput_AVX512::create();
        default:
            return nullptr;
    }
}
}  // namespace

extern "C" {

void* parpar_md5single_new(void) {
    configure_md5single();
    return new (std::nothrow) MD5Single();
}

void parpar_md5single_update(void* ctx, const unsigned char* data, size_t len) {
    if (!ctx || (!data && len != 0)) {
        return;
    }
    static_cast<MD5Single*>(ctx)->update(data, len);
}

void parpar_md5single_end(void* ctx, unsigned char* out) {
    if (!ctx || !out) {
        return;
    }
    static_cast<MD5Single*>(ctx)->end(out);
}

void parpar_md5single_free(void* ctx) {
    delete static_cast<MD5Single*>(ctx);
}

void parpar_md5single_hash(const unsigned char* data, size_t len, unsigned char* out) {
    if (!out || (!data && len != 0)) {
        return;
    }
    configure_md5single();
    MD5Single ctx;
    ctx.update(data, len);
    ctx.end(out);
}

void* parpar_hasher_input_new(unsigned method) {
    return create_input(static_cast<ParParHasherInputMethod>(method));
}

void parpar_hasher_input_update(void* ctx, const unsigned char* data, size_t len) {
    if (!ctx || (!data && len != 0)) {
        return;
    }
    static_cast<IHasherInput*>(ctx)->update(data, len);
}

void parpar_hasher_input_end(void* ctx, unsigned char* out) {
    if (!ctx || !out) {
        return;
    }
    static_cast<IHasherInput*>(ctx)->end(out);
}

void parpar_hasher_input_reset(void* ctx) {
    if (!ctx) {
        return;
    }
    static_cast<IHasherInput*>(ctx)->reset();
}

bool parpar_hasher_input_is_available(unsigned method) {
    switch (static_cast<ParParHasherInputMethod>(method)) {
        case PARPAR_HASHER_INPUT_SCALAR:
            return HasherInput_Scalar::isAvailable;
        case PARPAR_HASHER_INPUT_SIMD:
            return HasherInput_SSE::isAvailable;
        case PARPAR_HASHER_INPUT_CRC:
            return HasherInput_ClMulScalar::isAvailable;
        case PARPAR_HASHER_INPUT_SIMD_CRC:
            return HasherInput_ClMulSSE::isAvailable;
        case PARPAR_HASHER_INPUT_BMI1:
            return HasherInput_BMI1::isAvailable;
        case PARPAR_HASHER_INPUT_AVX512:
            return HasherInput_AVX512::isAvailable;
        default:
            return false;
    }
}

void parpar_hasher_input_free(void* ctx) {
    if (!ctx) {
        return;
    }
    static_cast<IHasherInput*>(ctx)->destroy();
}

bool parpar_hasher_input_hash(unsigned method, const unsigned char* data, size_t len, unsigned char* out) {
    if (!out || (!data && len != 0)) {
        return false;
    }

    IHasherInput* ctx = create_input(static_cast<ParParHasherInputMethod>(method));
    if (!ctx) {
        return false;
    }
    ctx->update(data, len);
    ctx->end(out);
    ctx->destroy();
    return true;
}

uint32_t parpar_crc32_compute(const unsigned char* data, size_t len) {
    if (!data && len != 0) {
        return 0;
    }
    return CRC32_Calc_ClMul(data, len);
}

}
