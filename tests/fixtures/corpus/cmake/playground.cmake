cmake_minimum_required(VERSION 3.20)
project(deploy_agent)

set(AGENT_TIMEOUT 45)

add_executable(agent main.c transport.c)
target_link_libraries(agent m)

function(enable_warnings target_name)
  target_compile_options(${target_name} PRIVATE -Wall -Wextra)
endfunction()

function(print_banner)
  message(STATUS "building the deploy agent")
endfunction()
