#include <algorithm>
#include <array>
#include <cstdint>
#include <cstdio>
#include <fstream>
#include <stdexcept>
#include <string>
#include <vector>

#include <unistd.h>

#include "gbi/rt64_gbi_f3d.h"
#include "gbi/rt64_gbi_f3dex2.h"
#include "gbi/rt64_gbi_rdp.h"
#include "hle/rt64_application.h"
#include "hle/rt64_present_queue.h"
#include "hle/rt64_workload_queue.h"

#define STB_IMAGE_WRITE_STATIC
#define STB_IMAGE_WRITE_IMPLEMENTATION
#include "stb/stb_image_write.h"

namespace {
constexpr uint32_t rdramSize = 8 * 1024 * 1024;

struct Task {
    uint32_t entry;
    std::string microcode;
    std::array<uint32_t, 16> segments;
};

struct Metadata {
    uint32_t width;
    uint32_t height;
    uint32_t address;
    uint32_t size;
    std::vector<Task> tasks;
};

uint32_t unsignedField(const json &object, const char *name) {
    const auto &value = object.at(name);
    if (!value.is_number_unsigned() || value.get<uint64_t>() > UINT32_MAX) {
        throw std::runtime_error(std::string(name) + " must be an unsigned 32-bit integer");
    }
    return value.get<uint32_t>();
}

RT64::GBIUCode microcodeId(const std::string &name) {
    if (name == "f3d") {
        return RT64::GBIUCode::F3D;
    }
    if (name == "f3dex2") {
        return RT64::GBIUCode::F3DEX2;
    }
    throw std::runtime_error("unsupported microcode: " + name + " (expected f3d or f3dex2)");
}

Metadata readMetadata(const std::string &path) {
    std::ifstream input(path);
    if (!input) {
        throw std::runtime_error("cannot open metadata: " + path);
    }
    const auto document = json::parse(input);
    if (unsignedField(document, "version") != 1) {
        throw std::runtime_error("unsupported metadata version (expected 1)");
    }
    Metadata metadata{};
    metadata.width = unsignedField(document, "width");
    metadata.height = unsignedField(document, "height");
    if (metadata.width == 0 || metadata.width > 1022 || metadata.height < 4 ||
        metadata.height > 512 || metadata.height % 4 != 0) {
        throw std::runtime_error("VI requires width 1..1022 and height 4..512 divisible by 4");
    }
    const auto &color = document.at("color_image");
    metadata.address = unsignedField(color, "address");
    metadata.size = unsignedField(color, "size");
    if (unsignedField(color, "format") != 0 || (metadata.size != 2 && metadata.size != 3)) {
        throw std::runtime_error("color_image must be RGBA16 (format 0, size 2) or RGBA32 (format 0, size 3)");
    }
    if (unsignedField(color, "width") != metadata.width) {
        throw std::runtime_error("color_image.width must match output width");
    }
    const uint64_t rowBytes = uint64_t(metadata.width) << (metadata.size - 1);
    if (metadata.address % 8 != 0 || rowBytes % 4 != 0 ||
        metadata.address + rowBytes * metadata.height > rdramSize) {
        throw std::runtime_error("color_image must fit RDRAM, use an 8-byte aligned address and a word-aligned stride");
    }
    const auto &tasks = document.at("tasks");
    if (!tasks.is_array() || tasks.empty()) {
        throw std::runtime_error("tasks must be a nonempty array");
    }
    for (const auto &value : tasks) {
        Task task{};
        task.entry = unsignedField(value, "entry");
        task.microcode = value.at("microcode").get<std::string>();
        microcodeId(task.microcode);
        if (task.entry % 8 != 0 || task.entry > rdramSize - 8) {
            throw std::runtime_error("task entry must be an aligned physical RDRAM address");
        }
        const auto &segments = value.at("segments");
        if (!segments.is_array() || segments.size() != task.segments.size()) {
            throw std::runtime_error("task segments must contain exactly 16 physical RDRAM addresses");
        }
        for (size_t i = 0; i < task.segments.size(); i++) {
            if (!segments[i].is_number_unsigned() || segments[i].get<uint64_t>() >= rdramSize) {
                throw std::runtime_error("task segment is outside RDRAM");
            }
            task.segments[i] = segments[i].get<uint32_t>();
        }
        metadata.tasks.push_back(task);
    }
    return metadata;
}

std::vector<uint8_t> readRdram(const std::string &path) {
    std::ifstream input(path, std::ios::binary | std::ios::ate);
    if (!input || input.tellg() != rdramSize) {
        throw std::runtime_error("RDRAM input must contain exactly 8388608 big-endian bytes");
    }
    std::vector<uint8_t> memory(rdramSize);
    input.seekg(0);
    if (!input.read(reinterpret_cast<char *>(memory.data()), memory.size())) {
        throw std::runtime_error("failed to read RDRAM input");
    }
    for (size_t i = 0; i < memory.size(); i += 4) {
        std::reverse(memory.begin() + i, memory.begin() + i + 4);
    }
    return memory;
}

void checkInterrupts() {}

struct CoreStorage {
    std::array<uint8_t, 64> header{};
    std::array<uint8_t, 4096> dmem{};
    std::array<uint8_t, 4096> imem{};
    std::array<uint32_t, 23> registers{};

    RT64::Application::Core core(std::vector<uint8_t> &memory, const Metadata &metadata) {
        RT64::Application::Core result{};
        result.HEADER = header.data();
        result.RDRAM = memory.data();
        result.DMEM = dmem.data();
        result.IMEM = imem.data();
        result.MI_INTR_REG = &registers[0];
        result.DPC_START_REG = &registers[1];
        result.DPC_END_REG = &registers[2];
        result.DPC_CURRENT_REG = &registers[3];
        result.DPC_STATUS_REG = &registers[4];
        result.DPC_CLOCK_REG = &registers[5];
        result.DPC_BUFBUSY_REG = &registers[6];
        result.DPC_PIPEBUSY_REG = &registers[7];
        result.DPC_TMEM_REG = &registers[8];
        result.VI_STATUS_REG = &registers[9];
        result.VI_ORIGIN_REG = &registers[10];
        result.VI_WIDTH_REG = &registers[11];
        result.VI_INTR_REG = &registers[12];
        result.VI_V_CURRENT_LINE_REG = &registers[13];
        result.VI_TIMING_REG = &registers[14];
        result.VI_V_SYNC_REG = &registers[15];
        result.VI_H_SYNC_REG = &registers[16];
        result.VI_LEAP_REG = &registers[17];
        result.VI_H_START_REG = &registers[18];
        result.VI_V_START_REG = &registers[19];
        result.VI_V_BURST_REG = &registers[20];
        result.VI_X_SCALE_REG = &registers[21];
        result.VI_Y_SCALE_REG = &registers[22];
        result.checkInterrupts = &checkInterrupts;
        *result.VI_STATUS_REG = metadata.size | (VI_STATUS_AA_MODE_NONE << 8);
        // RT64's VI decoder subtracts one scanline from the hardware origin.
        *result.VI_ORIGIN_REG = metadata.address + (metadata.width << (metadata.size - 1));
        *result.VI_WIDTH_REG = metadata.width;
        *result.VI_V_SYNC_REG = 525;
        *result.VI_H_SYNC_REG = 3093;
        *result.VI_H_START_REG = (1U << 16) | (metadata.width + 1);
        // fbSize adds two rows, then rounds to a multiple of four.
        *result.VI_V_START_REG = 2 * (metadata.height - 2);
        *result.VI_X_SCALE_REG = 1024;
        *result.VI_Y_SCALE_REG = 1024;
        return result;
    }
};

[[noreturn]] void rejectCommand(RT64::State *, RT64::DisplayList **dl) {
    char message[128];
    std::snprintf(message, sizeof(message), "unsupported GBI opcode 0x%02x: 0x%08x 0x%08x",
        (*dl)->w0 >> 24, (*dl)->w0, (*dl)->w1);
    throw std::runtime_error(message);
}

void selectMicrocode(RT64::Application &app, const Task &task, const std::string &override) {
    const std::string &name = override.empty() ? task.microcode : override;
    const auto id = microcodeId(name);
    auto &gbi = app.interpreter->gbiManager.gbiCache[static_cast<uint32_t>(id)];
    if (gbi.ucode == RT64::GBIUCode::Unknown) {
        // Mirror getGBIForUCode's cache initialization without requiring a ROM microcode blob.
        gbi.ucode = id;
        RT64::GBI_RDP::setup(&gbi, true);
        if (id == RT64::GBIUCode::F3D) {
            RT64::GBI_F3D::setup(&gbi);
        }
        else {
            RT64::GBI_F3DEX2::setup(&gbi);
        }
        // Release RT64 silently skips unmapped commands because its log macro is disabled.
        for (auto &handler : gbi.map) {
            if (handler == nullptr) {
                handler = &rejectCommand;
            }
        }
    }
    gbi.flags = {};
    app.interpreter->hleGBI = &gbi;
    app.state->rsp->setGBI(&gbi);
    if (gbi.resetFromTask != nullptr) {
        gbi.resetFromTask(app.state.get());
    }
    app.state->rsp->segments = task.segments;
    std::fprintf(stderr, "GBI %s, entry 0x%08x, standard flags\n", name.c_str(), task.entry);
}

struct ApplicationEnd {
    RT64::Application &app;
    ~ApplicationEnd() { app.end(); }
};

void render(std::vector<uint8_t> &memory, const Metadata &metadata, const std::string &gbi, uint32_t scale) {
    CoreStorage storage;
    RT64::ApplicationConfiguration configuration;
    configuration.detectDataPath = false;
    configuration.useConfigurationFile = false;
    RT64::Application app(storage.core(memory, metadata), configuration);
    ApplicationEnd end{app};
    app.emulatorConfig.framebuffer.renderToRAM = true;
    app.userConfig.resolution = RT64::UserConfiguration::Resolution::Original;
    app.userConfig.resolutionMultiplier = 1.0;
    app.userConfig.downsampleMultiplier = 1;
    app.userConfig.antialiasing = RT64::UserConfiguration::Antialiasing::None;
    app.userConfig.aspectRatio = RT64::UserConfiguration::AspectRatio::Original;
    app.userConfig.extAspectRatio = RT64::UserConfiguration::AspectRatio::Original;
    app.userConfig.refreshRate = RT64::UserConfiguration::RefreshRate::Original;
    app.userConfig.internalColorFormat = RT64::UserConfiguration::InternalColorFormat::Standard;
    app.userConfig.idleWorkActive = false;
    app.enhancementConfig.presentation.mode = RT64::EnhancementConfiguration::Presentation::Mode::Console;
    std::fprintf(stderr, "setup: native %ux%u, window scale %u, renderToRAM enabled\n",
        metadata.width, metadata.height, scale);
    const auto setup = app.setup(0);
    std::fprintf(stderr, "setup result: %d\n", static_cast<int>(setup));
    if (setup != RT64::Application::SetupResult::Success) {
        throw std::runtime_error("RT64 setup failed: " + RT64::GlobalLastError);
    }
    if (app.appWindow->sdlWindow == nullptr) {
        throw std::runtime_error(std::string("RT64 did not create an SDL window: ") + SDL_GetError());
    }
    SDL_SetWindowSize(app.appWindow->sdlWindow, metadata.width * scale, metadata.height * scale);
    SDL_PumpEvents();
    for (const auto &task : metadata.tasks) {
        selectMicrocode(app, task, gbi);
        app.processDisplayLists(memory.data(), task.entry, 0, true);
        // A task may omit FullSync or emit commands after its last FullSync.
        app.state->fullSync();
        app.state->dpInterrupt();
        app.workloadQueue->waitForWorkloadId(app.state->workloadId);
        std::fprintf(stderr, "completed workload %llu\n", static_cast<unsigned long long>(app.state->workloadId));
    }
    const auto &color = app.state->rdp->colorImage;
    if (color.address != metadata.address || color.width != metadata.width || color.fmt != 0 || color.siz != metadata.size) {
        throw std::runtime_error("final RT64 color image disagrees with metadata");
    }
    app.updateScreen();
    app.workloadQueue->waitForWorkloadId(app.state->workloadId);
    app.presentQueue->waitForPresentId(app.state->presentId);
    app.workloadQueue->waitForIdle();
    app.presentQueue->waitForIdle();
    std::fprintf(stderr, "completed presentation %llu\n", static_cast<unsigned long long>(app.state->presentId));
}

void writeImages(const std::vector<uint8_t> &memory, const Metadata &metadata, const std::string &prefix) {
    const size_t pixels = size_t(metadata.width) * metadata.height;
    std::vector<uint8_t> rgba(pixels * 4);
    for (size_t i = 0; i < pixels; i++) {
        const size_t address = metadata.address + (i << (metadata.size - 1));
        if (metadata.size == 2) {
            const uint16_t pixel = (uint16_t(memory[address ^ 3]) << 8) | memory[(address + 1) ^ 3];
            for (size_t channel = 0; channel < 3; channel++) {
                const uint8_t value = (pixel >> (11 - channel * 5)) & 31;
                rgba[i * 4 + channel] = (value << 3) | (value >> 2);
            }
            rgba[i * 4 + 3] = (pixel & 1) ? 255 : 0;
        }
        else {
            for (size_t channel = 0; channel < 4; channel++) {
                rgba[i * 4 + channel] = memory[(address + channel) ^ 3];
            }
        }
    }
    std::ofstream output(prefix + ".rgba8", std::ios::binary);
    if (!output.write(reinterpret_cast<const char *>(rgba.data()), rgba.size())) {
        throw std::runtime_error("cannot write RGBA8 output");
    }
    output.close();
    if (!output || !stbi_write_png((prefix + ".png").c_str(), metadata.width, metadata.height,
        4, rgba.data(), metadata.width * 4)) {
        throw std::runtime_error("failed to write output images");
    }
    std::fprintf(stderr, "wrote %s.rgba8 and %s.png\n", prefix.c_str(), prefix.c_str());
}
}

int main(int argc, char **argv) {
    if (argc < 4) {
        std::fprintf(stderr, "usage: rt64-oracle <out>.rdram <out>.json <prefix> [--gbi f3d|f3dex2] [--scale N]\n");
        return 1;
    }
    const std::string prefix = argv[3];
    std::fprintf(stderr, "RT64 diagnostics: %s.log\n", prefix.c_str());
    if (std::freopen((prefix + ".log").c_str(), "w", stderr) == nullptr) {
        return 1;
    }
    if (dup2(fileno(stderr), fileno(stdout)) < 0) {
        std::fprintf(stderr, "cannot redirect RT64 stdout\n");
        return 1;
    }
    std::setvbuf(stderr, nullptr, _IONBF, 0);
#ifndef NDEBUG
    RT64::GlobalLogFile = stderr;
#endif
    try {
        std::string gbi;
        uint32_t scale = 1;
        bool scaleSet = false;
        for (int i = 4; i < argc; i += 2) {
            if (i + 1 == argc) {
                throw std::runtime_error("missing option value");
            }
            const std::string option = argv[i];
            const std::string value = argv[i + 1];
            if (option == "--gbi" && gbi.empty()) {
                microcodeId(value);
                gbi = value;
            }
            else if (option == "--scale" && !scaleSet) {
                if (value.empty() || value.find_first_not_of("0123456789") != std::string::npos) {
                    throw std::runtime_error("scale must be an integer in 1..16");
                }
                const auto parsed = std::stoul(value);
                if (parsed < 1 || parsed > 16) {
                    throw std::runtime_error("scale must be an integer in 1..16");
                }
                scale = static_cast<uint32_t>(parsed);
                scaleSet = true;
            }
            else {
                throw std::runtime_error("unknown or repeated option: " + option);
            }
        }
        const auto metadata = readMetadata(argv[2]);
        auto memory = readRdram(argv[1]);
        render(memory, metadata, gbi, scale);
        writeImages(memory, metadata, prefix);
        return 0;
    }
    catch (const std::exception &error) {
        std::fprintf(stderr, "error: %s\n", error.what());
        return 1;
    }
}
