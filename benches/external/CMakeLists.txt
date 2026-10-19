# The external engines of the suite (see scripts/external.sh, which fetches their pinned sources).
cmake_minimum_required(VERSION 3.16)
project(cephal_external C CXX)
set(CMAKE_CXX_STANDARD 20)
set(CMAKE_C_STANDARD 11)
if(NOT CMAKE_BUILD_TYPE)
  set(CMAKE_BUILD_TYPE Release)
endif()
# -O3 without -march=native or -ffast-math, like the Rust bench profile; CLAY_FAST_MATH reproduces
# the flags PanGui published Clay's numbers with.
set(CMAKE_C_FLAGS_RELEASE "-O3 -DNDEBUG")
set(CMAKE_CXX_FLAGS_RELEASE "-O3 -DNDEBUG")
set(YOGA_SRC "" CACHE PATH "Yoga checkout")
set(CLAY_SRC "" CACHE PATH "Clay checkout")
option(CLAY_FAST_MATH "Build Clay with -ffast-math" OFF)

add_subdirectory(${YOGA_SRC}/yoga ${CMAKE_BINARY_DIR}/yoga)
# Yoga's defaults append -O2 after the build type's flags; PanGui's published numbers used -O3.
target_compile_options(yogacore PRIVATE -O3)
include(CheckIPOSupported)
check_ipo_supported(RESULT ipo)
add_executable(yoga_bench yoga_bench.cpp)
target_include_directories(yoga_bench PRIVATE ${CMAKE_CURRENT_SOURCE_DIR})
target_link_libraries(yoga_bench PRIVATE yogacore m)

add_executable(clay_bench clay_bench.c)
target_include_directories(clay_bench PRIVATE ${CLAY_SRC} ${CMAKE_CURRENT_SOURCE_DIR})
target_link_libraries(clay_bench PRIVATE m)
if(CLAY_FAST_MATH)
  target_compile_options(clay_bench PRIVATE -ffast-math)
endif()
if(ipo)
  set_target_properties(yoga_bench clay_bench PROPERTIES INTERPROCEDURAL_OPTIMIZATION ON)
endif()
