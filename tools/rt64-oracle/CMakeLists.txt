cmake_minimum_required(VERSION 3.20)
project(rt64_oracle LANGUAGES C CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_CXX_EXTENSIONS OFF)
if(NOT CMAKE_BUILD_TYPE)
    set(CMAKE_BUILD_TYPE Release CACHE STRING "Build type" FORCE)
endif()

set(RT64_ROOT "" CACHE PATH "Read-only RT64 reference checkout")
set(RT64_BUILD_DIR "${RT64_ROOT}/build" CACHE PATH "Existing RT64_STATIC=ON build")
if(NOT EXISTS "${RT64_ROOT}/src/hle/rt64_application.h")
    message(FATAL_ERROR "Set RT64_ROOT to the RT64 reference checkout")
endif()
if(NOT APPLE)
    message(FATAL_ERROR "This harness currently links the macOS RT64 static build")
endif()

find_package(SDL2 REQUIRED)
find_package(ZLIB REQUIRED)
find_package(Threads REQUIRED)

add_executable(rt64-oracle main.cpp "${RT64_ROOT}/src/contrib/xxHash/xxhash.c")
target_compile_definitions(rt64-oracle PRIVATE HLSL_CPU FFX_GCC IMGUI_IMPL_VULKAN_NO_PROTOTYPES)
target_include_directories(rt64-oracle SYSTEM PRIVATE
    ${SDL2_INCLUDE_DIRS}
    "${RT64_ROOT}/src"
    "${RT64_ROOT}/src/contrib"
    "${RT64_ROOT}/src/contrib/plume"
    "${RT64_ROOT}/src/contrib/imgui"
    "${RT64_ROOT}/src/contrib/hlslpp/include"
    "${RT64_ROOT}/src/contrib/mupen64plus-core/src/api"
    "${RT64_ROOT}/src/contrib/nativefiledialog-extended/src/include"
    "${RT64_ROOT}/src/contrib/metal-cpp"
    "${RT64_ROOT}/src/contrib/plume/contrib/Vulkan-Headers/include"
)

foreach(archive IN ITEMS
    rt64.a
    src/contrib/re-spirv/libre-spirv.a
    src/contrib/nativefiledialog-extended/src/libnfd.a
    src/contrib/zstd/build/cmake/lib/libzstd.a
    src/contrib/plume/libplume.a
)
    if(NOT EXISTS "${RT64_BUILD_DIR}/${archive}")
        message(FATAL_ERROR "Missing ${RT64_BUILD_DIR}/${archive}; build RT64_STATIC=ON first")
    endif()
    target_link_libraries(rt64-oracle PRIVATE "${RT64_BUILD_DIR}/${archive}")
endforeach()

target_link_libraries(rt64-oracle PRIVATE ${SDL2_LIBRARIES} ZLIB::ZLIB Threads::Threads)
foreach(framework IN ITEMS Metal QuartzCore CoreGraphics IOKit AppKit Foundation UniformTypeIdentifiers)
    find_library(${framework}_LIBRARY ${framework} REQUIRED)
    target_link_libraries(rt64-oracle PRIVATE "${${framework}_LIBRARY}")
endforeach()
