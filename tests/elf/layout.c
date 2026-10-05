#include <elf.h>
#include <stddef.h>

#if __BYTE_ORDER__ == __ORDER_BIG_ENDIAN__
#define ELF_NATIVE_DATA ELFDATA2MSB
#define LOW_BYTE(n, width) ((n) + (width) - 1)
#else
#define ELF_NATIVE_DATA ELFDATA2LSB
#define LOW_BYTE(n, width) (n)
#endif

/* ELF gABI, Table 1.2 and the ELF64 header layouts. */
_Static_assert(sizeof(Elf64_Word) == 4, "ELF64 Word width");
_Static_assert(sizeof(Elf64_Sword) == 4, "ELF64 Sword width");
_Static_assert(_Alignof(Elf64_Word) == 4, "ELF64 Word alignment");
_Static_assert(_Alignof(Elf64_Sword) == 4, "ELF64 Sword alignment");
_Static_assert((Elf64_Sword)-1 < 0, "ELF64 Sword signedness");
_Static_assert((Elf64_Word)-1 > 0, "ELF64 Word signedness");
_Static_assert(sizeof(Elf64_Xword) == 8, "ELF64 Xword width");
_Static_assert(sizeof(Elf64_Sxword) == 8, "ELF64 Sxword width");
_Static_assert(sizeof(Elf64_Ehdr) == 64, "ELF64 file header");
_Static_assert(sizeof(Elf64_Phdr) == 56, "ELF64 program header");
_Static_assert(sizeof(Elf64_Shdr) == 64, "ELF64 section header");
_Static_assert(sizeof(Elf64_Sym) == 24, "ELF64 symbol");
_Static_assert(sizeof(Elf64_Rel) == 16, "ELF64 relocation");
_Static_assert(sizeof(Elf64_Rela) == 24, "ELF64 relocation with addend");
_Static_assert(sizeof(Elf64_Dyn) == 16, "ELF64 dynamic entry");
_Static_assert(sizeof(Elf64_Nhdr) == 12, "ELF64 note header");
_Static_assert(offsetof(Elf64_Ehdr, e_version) == 20, "ELF64 version offset");
_Static_assert(offsetof(Elf64_Ehdr, e_entry) == 24, "ELF64 entry offset");
_Static_assert(offsetof(Elf64_Ehdr, e_phoff) == 32, "ELF64 program table offset");
_Static_assert(offsetof(Elf64_Ehdr, e_flags) == 48, "ELF64 flags offset");
_Static_assert(offsetof(Elf64_Ehdr, e_phentsize) == 54, "ELF64 entry size offset");
_Static_assert(offsetof(Elf64_Ehdr, e_phnum) == 56, "ELF64 entry count offset");
_Static_assert(offsetof(Elf64_Phdr, p_flags) == 4, "ELF64 program flags offset");
_Static_assert(offsetof(Elf64_Phdr, p_offset) == 8, "ELF64 program file offset");
_Static_assert(offsetof(Elf64_Sym, st_info) == 4, "ELF64 symbol info offset");
_Static_assert(offsetof(Elf64_Sym, st_value) == 8, "ELF64 symbol value offset");

/* The caller can also pass a real ELF file to this header-only probe. */
int elf64_header_layout(const unsigned char *bytes, size_t length) {
    if (length < sizeof(Elf64_Ehdr)) {
        return 1;
    }
    Elf64_Ehdr header;
    unsigned char *out = (unsigned char *)&header;
    for (size_t i = 0; i < sizeof(header); ++i) {
        out[i] = bytes[i];
    }
    return header.e_ident[0] != 0x7f || header.e_ident[1] != 'E' ||
           header.e_ident[2] != 'L' || header.e_ident[3] != 'F' ||
           header.e_ident[EI_CLASS] != ELFCLASS64 ||
           header.e_ident[EI_DATA] != ELF_NATIVE_DATA || header.e_version != 1 ||
           header.e_ehsize != sizeof(Elf64_Ehdr) ||
           header.e_phentsize != sizeof(Elf64_Phdr) || header.e_phnum == 0;
}

int main(void) {
    const unsigned char file[64] = {
        [0] = 0x7f, [1] = 'E', [2] = 'L', [3] = 'F',
        [EI_CLASS] = ELFCLASS64, [EI_DATA] = ELF_NATIVE_DATA,
        [LOW_BYTE(20, 4)] = 1, [LOW_BYTE(52, 2)] = 64,
        [LOW_BYTE(54, 2)] = 56, [LOW_BYTE(56, 2)] = 7,
    };
    return elf64_header_layout(file, sizeof(file));
}
