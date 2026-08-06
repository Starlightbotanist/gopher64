cmake_minimum_required(VERSION 3.22.1)
project(gopher64_android_dependencies LANGUAGES C CXX)

if(CMAKE_ANDROID_ARCH_ABI STREQUAL "arm64-v8a")
    set(BUILD_SHARED_LIBS ON CACHE BOOL "" FORCE)
    set(GEN_INSTALL_TARGET OFF CACHE BOOL "" FORCE)
    add_subdirectory(
        "${CMAKE_CURRENT_LIST_DIR}/../../../../../external/libadrenotools"
        "${CMAKE_CURRENT_BINARY_DIR}/libadrenotools"
    )
endif()
