section .data
    msg db "Hello, World!", 10
    len equ $ - msg
section .text
    global _start
print_msg:
    mov rax, 1
    mov rdi, 1
    lea rsi, [msg]
    mov rdx, len
    syscall
    ret
_start:
    call print_msg
    mov rax, 60
    xor rdi, rdi
    syscall

scratch_pad:
    mov r8, 77
    ret

drain_queue:
    mov r9, 88
    ret
