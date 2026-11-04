#include <algorithm>
#include <array>
#include <cassert>
#include <cstdint>
#include <fstream>
#include <iomanip>
#include <iostream>
#include <sstream>
#include <vector>
#define XXH_STATIC_LINKING_ONLY
#define XXH_IMPLEMENTATION
#include "contrib/xxHash/xxhash.h"
#include "common/rt64_load_types.h"
#include "shared/rt64_f3d_defines.h"

static std::vector<uint8_t> stream;
static std::vector<std::pair<size_t, size_t>> spans;
static const uint8_t *bank;
static XXH_errorcode record_update(XXH3_state_t *state, const void *input, size_t size) {
    auto bytes = static_cast<const uint8_t *>(input);
    stream.insert(stream.end(), bytes, bytes + size);
    auto address = reinterpret_cast<uintptr_t>(input);
    auto base = reinterpret_cast<uintptr_t>(bank);
    if (address >= base && address + size <= base + 4096) {
        spans.emplace_back(address - base, size);
    }
    return XXH3_64bits_update(state, input, size);
}
#define XXH3_64bits_update record_update
#include "common/rt64_tmem_hasher.h"
#undef XXH3_64bits_update

static std::string hex(const std::vector<uint8_t> &bytes) {
    std::ostringstream result;
    for (auto byte : bytes) result << std::hex << std::setfill('0') << std::setw(2) << unsigned(byte);
    return result.str();
}

int main(int argc, char **argv) {
    static_assert(RT64::TMEMHasher::CurrentHashVersion == 5);
    const uint16_t endian = 1;
    if (*reinterpret_cast<const uint8_t *>(&endian) != 1 || argc != 2) return 2;
    std::ifstream file(argv[1]);
    auto cases = json::parse(file);
    json output = json::array();
    for (const auto &item : cases) {
        std::array<uint8_t, 4096> tmem {};
        auto encoded = item.at("bank").get<std::string>();
        if (encoded.size() != 8192) return 3;
        for (size_t i = 0; i < tmem.size(); i++) tmem[i] = std::stoul(encoded.substr(i * 2, 2), nullptr, 16);
        RT64::LoadTile tile {};
        tile.fmt = item.at("fmt"); tile.siz = item.at("siz");
        tile.line = item.at("line"); tile.tmem = item.at("base"); tile.palette = item.at("palette");
        uint16_t width = item.at("width"), height = item.at("height");
        uint32_t tlut = item.at("tlut").get<uint32_t>() << 14;
        bank = tmem.data(); stream.clear(); spans.clear();
        auto digest = RT64::TMEMHasher::hash(bank, tile, width, height, tlut, 5);
        if (digest != XXH3_64bits(stream.data(), stream.size())) return 4;
        std::ostringstream hash;
        hash << std::hex << std::setfill('0') << std::setw(16) << digest;
        output.push_back({{"name", item.at("name")}, {"xxh3_64", hash.str()}, {"stream", hex(stream)}, {"spans", spans}});
    }
    std::cout << output.dump(2) << '\n';
}
