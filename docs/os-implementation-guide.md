# Rust xv6 作業系統實作導覽

> 一份寫給「剛開始學作業系統」的讀者的導覽,帶你走過一個真實(雖然精簡)的作業系統核心是如何實作出來的。

## 前言

作業系統的課本常常停在概念層次:分頁、行程、排程、檔案系統……每個名詞都懂,但「它到底是怎麼被寫出來的?」往往還是一團迷霧。這份文件的目的,就是把概念和**真實可以編譯、可以開機、可以跑程式**的程式碼接起來。

我們的教材是 **xv6**——由 MIT 為教學而設計的迷你 Unix-like 作業系統。原始的 xv6 用 C 寫成,執行在 RISC-V 架構上。本專案(分支 `rust-rewrite`)則是把這個核心**改寫成 Rust**,同時盡量維持與原版一致的行為:相同的系統呼叫編號、相同的使用者程式二進位格式、相同的磁碟檔案系統佈局。

為什麼用 Rust 重寫值得一讀?因為作業系統核心是「與硬體搏鬥、充滿 `unsafe` 操作」的地方,而 Rust 的型別系統與所有權模型,讓我們能把大部分危險操作用安全的抽象包起來,只在少數角落(組合語言、頁框配置、鎖)才觸碰 `unsafe`。閱讀時你會不斷看到「同一件事,C 怎麼做、Rust 怎麼做」的對照——這本身就是一堂很好的系統程式設計課。

### 這份文件涵蓋什麼

一個作業系統核心通常由幾大子系統組成,它們彼此協作:

- **虛擬記憶體**——讓每個程式以為自己獨佔整台機器的記憶體。
- **陷阱與系統呼叫**——使用者程式如何「請求」核心幫忙,以及硬體如何強制進入核心。
- **行程管理與排程**——核心如何建立、切換、回收正在執行的程式。
- **檔案系統**——資料如何在斷電也不遺失的前提下,有結構地存放在磁碟上。
- **同步、IPC 與裝置驅動**——多核心之下如何安全共享資料、行程之間如何溝通、核心如何驅動硬體。

### 如何閱讀

本文件由淺入深,建議照順序閱讀:先建立**記憶體**的基礎,再理解 CPU 如何**進入核心(trap)**,接著看核心裡實際**執行的單位(行程)**,然後是**持久化的資料(檔案系統)**,最後是把這一切黏合起來的**同步機制、IPC 與驅動程式**。

每一章都會先講「這是什麼、為什麼需要它」的作業系統概念,再對應到本專案的實際程式碼(標註 `檔案:行號`,方便你打開原始碼對照)。你不需要先讀懂全部程式碼——先抓住概念與資料流,再回頭鑽細節。

> **執行環境**:程式碼以 nightly Rust 編譯,目標平台 `riscv64imac-unknown-none-elf`,在 QEMU 的 `-machine virt` 上開機。核心原始碼位於 `kernel/src/`。

### 目錄

- [第 0 章:開機與整體架構](#第-0-章開機與整體架構)
- [第 1 章:虛擬記憶體](#虛擬記憶體virtual-memory)
- [第 2 章:陷阱與系統呼叫](#陷阱與系統呼叫traps--system-calls)
- [第 3 章:Process 管理與排程](#process-管理與排程processes--scheduling)
- [第 4 章:檔案系統](#檔案系統file-system)
- [第 5 章:同步、IPC 與裝置驅動](#同步ipc-與裝置驅動synchronization-ipc--drivers)
- [結語:子系統如何協作](#結語子系統如何協作)

---

## 第 0 章:開機與整體架構

在深入各子系統之前,我們先站在高處鳥瞰:從按下電源到跑起第一個使用者程式,核心到底做了哪些事?這一章像一張地圖,之後每一章都是地圖上某個區域的放大。

### 核心的模組地圖

本專案的核心程式碼(`kernel/src/`)依子系統切成幾個 Rust 模組(見 `kernel/src/lib.rs`):

| 模組 | 職責 | 對應本文章節 |
|------|------|--------------|
| `arch/` | RISC-V 架構相關:開機、組合語言、trap 進出、CSR 暫存器、PLIC 中斷控制器 | 第 0、2 章 |
| `mm/` | 記憶體管理:位址型別、頁框配置器、Sv39 頁表、核心 heap、page fault | 第 1 章 |
| `proc/` | 行程與排程:`Proc` 結構、排程器、trapframe、系統呼叫實作 | 第 2、3 章 |
| `fs/` | 檔案系統:buffer cache、write-ahead log、inode、檔案描述子、pipe | 第 4、5 章 |
| `sync/` | 同步原語:spinlock、sleeplock、條件變數 | 第 5 章 |
| `drivers/` | 裝置驅動:UART console、virtio 磁碟 | 第 5 章 |

一個設計原則貫穿全書:**把 `unsafe` 隔離在少數模組**(`arch/`、`mm/` 的頁框配置、`sync/`),讓其餘核心程式碼建立在安全的抽象之上。這正是 Rust 重寫最想展示的價值。

### 三種特權模式

RISC-V 有三種特權等級,開機過程會依序「降級」:

- **M-mode(Machine)**:權限最高,開機時 CPU 的起始模式,用來做一次性的機器層設定。
- **S-mode(Supervisor)**:作業系統核心執行的模式,能操作頁表(`satp`)、處理 trap。
- **U-mode(User)**:使用者程式執行的模式,權限最低,碰不到硬體與核心資料。

作業系統核心活在 S-mode,使用者程式活在 U-mode;M-mode 只在開機時短暫出現。

### 從電源到第一個程式

開機的資料流大致如下(對照 `kernel/src/arch/entry.S` 與 `kernel/src/arch/init.rs`):

1. **`_start`(組合語言,M-mode)**——每個 CPU 核心(hart)都從這裡進入。它先給每個 hart 一塊各自的開機堆疊(`sp = _stack_end - mhartid × 16384`,見 `entry.S:10`),避免多核互相踩踏,然後 `call mstart`。

2. **`mstart()`(`arch/init.rs:21`,M-mode)**——做一次性的機器層設定:把 `mstatus.MPP` 設成 S-mode、把 `mepc` 指向 `init`、把所有例外與中斷**委派(delegate)**給 S-mode(`medeleg`/`mideleg`)、設定 PMP 讓 S-mode 能存取全部實體記憶體、開啟 Sstc 讓 S-mode 能自行設定計時器、把 hart id 存進 `tp` 暫存器,最後 `mret` 跳進 S-mode 的 `init()`。這一步是關鍵:唯有進了 S-mode,`satp` 才生效、分頁與使用者模式才可能運作。

3. **`init()`(`arch/init.rs:66`,S-mode)**——所有 hart 都會執行,但用一個 `STARTED` 旗標協調:只有 **hart 0** 做完整初始化,其他 hart 先忙等,等 hart 0 完成後才繼續。hart 0 的初始化順序本身就是一張「子系統依賴圖」:

   ```
   consoleinit()      // 主控台(UART),讓核心能印字
   kinit()            // 頁框配置器:管理 _kernel_end 到 PHYSTOP 的實體記憶體    → 第 1 章
   kvminit()/kvminithart()  // 建立並啟用核心頁表(從此分頁生效)                 → 第 1 章
   procinit()         // 行程表初始化                                          → 第 3 章
   trapinit()         // 設定核心 trap 向量與第一次計時器中斷                    → 第 2 章
   plic_init()        // 中斷控制器                                            → 第 5 章
   virtio_init()      // virtio 磁碟驅動                                        → 第 5 章
   fsinit()           // 檔案系統:buffer cache、inode、log 復原                → 第 4 章
   fileinit()         // 檔案描述子層                                          → 第 4 章
   userinit()         // 建立第一個使用者行程(執行 init)                      → 第 3 章
   ```

   注意這個順序有其必然:必須先有頁框配置器,`kvminit` 才有頁可配;必須先有 virtio 驅動,檔案系統的 log 復原才讀得到磁碟;必須所有子系統就緒,才能生出第一個使用者行程。

4. **`scheduler()`(`arch/init.rs:122`)**——每個 hart 最後都進入排程器主迴圈,開始挑選可執行的行程來跑。第一個被跑起來的,就是 `userinit()` 建立的 `init` 程式,它接著會啟動 shell,系統就「活」了。

### 記憶體佈局速寫

核心被載入到實體位址 `0x80000000`(由連結腳本 `kernel/memory.x` 決定)。從核心映像結尾(`_kernel_end`)到 `PHYSTOP` 之間的實體記憶體,交給頁框配置器當作可分配的「空閒頁池」。虛擬位址空間的最頂端保留給 **trampoline** 頁(使用者與核心切換時的共用跳板,詳見第 2 章)。硬體裝置則以 **MMIO** 映射在低位址:UART 主控台在 `0x10000000`、virtio 磁碟在 `0x10001000`(第 5 章)。

有了這張地圖,我們就可以開始放大每個區域了。第 1 章先從最底層的基礎——虛擬記憶體——開始。

---

## 虛擬記憶體(Virtual Memory)

### 為什麼需要虛擬記憶體

如果每個 process 都直接讀寫實體記憶體(physical memory),就必須自己協調誰能用哪一段位址,一個 process 寫壞了另一個 process 的資料也無從防範。虛擬記憶體(virtual memory)替每個 process 建立一份「私有的位址空間幻覺」:process 看到的位址(virtual address,虛擬位址)與 CPU 實際存取的位址(physical address,實體位址)是兩套獨立的座標系統,中間由硬體的記憶體管理單元(MMU)配合作業系統維護的**頁表(page table)**做轉換。

好處有兩個:

1. **隔離(isolation)**:process A 的虛擬位址 `0x1000` 與 process B 的虛擬位址 `0x1000` 可以被映射(map)到完全不同的實體頁框,兩者互不可見、互不可寫。
2. **彈性佈局**:每個 process 都可以認為自己從位址 `0` 開始擁有一塊連續、私有的記憶體,不用管實際的實體記憶體長什麼樣、有沒有被別人占用。

xv6 把這件事交給 kernel 全權管理:kernel 自己也活在一個虛擬位址空間裡(kernel 的位址空間長期採 identity mapping,虛擬位址等於實體位址,方便直接用指標存取實體記憶體),而每個 user process 各自有一份獨立頁表。

### 分頁與 RISC-V Sv39

分頁(paging)把位址空間切成固定大小的**頁(page)**——這裡是 4KB——並用一張表記錄「虛擬頁 → 實體頁」的對應關係。RISC-V 64 位元定義了幾種分頁模式,xv6-riscv(包含這份 Rust 重寫)採用 **Sv39**:只用 64 位元位址裡的低 39 位元做轉換,也就是最多支援 512GB 的虛擬位址空間,對教學用 OS 綽綽有餘。

Sv39 把這 39 位元切成三段各 9 位元的索引(`VPN[2]`、`VPN[1]`、`VPN[0]`)加上 12 位元的頁內偏移量(page offset,對應 4KB = 2^12):

```
38          30 29          21 20          12 11         0
[   VPN[2]   ][   VPN[1]   ][   VPN[0]   ][  offset(12) ]
```

轉址是**三級查找**:先用 `VPN[2]` 在根頁表(root page table)裡找一個項目(page table entry,PTE),這個 PTE 指向第二級頁表;再用 `VPN[1]` 在第二級頁表裡找,指向第三級頁表;最後用 `VPN[0]` 在第三級頁表裡找到真正指向實體頁框的 PTE,加上 `offset` 就是最終的實體位址。

一個很好的類比是**多層字典查找**:VPN[2] 像是先翻到字典的某個字母分冊,VPN[1] 再翻到該分冊裡的某一頁範圍,VPN[0] 才翻到精確的那一頁——每一級都只需要記住「下一級在哪裡」,不用一次性列出所有可能的位址對應,大幅節省頁表本身佔用的記憶體。

每個 PTE 是 64 位元,除了存放下一級(或最終)實體頁框號(PPN)之外,還帶有一組**權限旗標(flags)**,這些常數定義在 `kernel/src/arch/registers.rs:48-55`:

```rust
pub const PTE_V: u64 = 1 << 0;  // Valid:此項目是否有效
pub const PTE_R: u64 = 1 << 1;  // 可讀
pub const PTE_W: u64 = 1 << 2;  // 可寫
pub const PTE_X: u64 = 1 << 3;  // 可執行
pub const PTE_U: u64 = 1 << 4;  // user mode 可存取(否則只有 kernel 能碰)
pub const PTE_A: u64 = 1 << 6;  // Accessed(已被存取過)
pub const PTE_D: u64 = 1 << 7;  // Dirty(已被寫入過)
```

`R/W/X` 全為 0 時,代表這個 PTE 不是「葉節點(leaf)」,而是指向下一級頁表的中繼節點;`R/W/X` 只要有一個不為 0,就代表這個 PTE 是葉節點,直接對應到一個實體頁。`kernel/src/mm/page_table.rs:180-193` 的 `free_walk` 正是靠這個規則判斷該遞迴進下一級,還是直接釋放實體頁。

CPU 要用哪一份頁表,是透過 **`satp`(Supervisor Address Translation and Protection)** 這顆控制暫存器(CSR)切換的:寫入 `satp` 等於告訴 MMU「現在開始用這棵頁表轉址」。`kernel/src/arch/asm.rs:196-199` 定義了組出 `satp` 值的函式:

```rust
pub const fn make_satp(ppn: usize) -> usize {
    // Sv39: MODE(8) 放在 bits [63:60],PPN 放在 bits [43:0]
    (8 << 60) | ppn
}
```

`8` 是 Sv39 模式碼(mode)。切換頁表後,MMU 內部的位址轉換快取(TLB,Translation Lookaside Buffer)可能還留著舊頁表的對應,必須用 `sfence.vma` 指令把它沖掉,否則會讀到過期的映射。`kernel/src/arch/asm.rs:162-166`:

```rust
pub fn sfence_vma() {
    unsafe { asm!("sfence.vma") };
}
```

### Rust 型別安全的位址表示:告別裸 `uint64`

原始 C xv6 裡,虛擬位址、實體位址、頁框號全都用同一種型別 `uint64` 表示,編譯器完全分不出你手上這個數字究竟代表什麼意義——把虛擬位址誤傳給一個該收實體位址的函式,C 編譯器不會有任何警告,只有跑起來才會出錯(甚至不一定馬上出錯)。

這份 Rust 重寫在 `kernel/src/mm/address.rs:14-40` 用四個 newtype 包裝同一個底層的 `usize`,把「這是什麼位址」變成編譯期就能檢查的事:

```rust
#[repr(transparent)]
pub struct PhysAddr(pub usize);

#[repr(transparent)]
pub struct VirtAddr(pub usize);

#[repr(transparent)]
pub struct PhysPageNum(pub usize);

#[repr(transparent)]
pub struct VirtPageNum(pub usize);
```

`#[repr(transparent)]` 保證這些型別在記憶體佈局上就是一個 `usize`,執行期沒有額外開銷——型別安全完全發生在編譯期。四種型別互不相容,`fn map(vaddr: VirtAddr, paddr: PhysAddr, ...)` 這樣的函式簽名,若呼叫端不小心把兩個參數順序搞反,編譯器會直接報型別錯誤,而不是讓 bug 潛伏到執行期才爆炸。

每個型別還附帶了語意明確的轉換方法,例如 `PhysAddr::floor()`/`ceil()` 做頁對齊捨去/進位(`kernel/src/mm/address.rs:47-50`)、`PhysPageNum::to_paddr()` 換算成頁框基底位址(`address.rs:75`)。這些方法名稱直接說明了「往下取整到頁邊界」或「頁框號轉頁基底位址」的意圖,比起 C 版本裡到處出現的 `PGROUNDDOWN(x)` 巨集搭配裸指標運算,可讀性與安全性都高出一截。

### 實體頁框配置器(Frame Allocator)

Kernel 需要一個機制,追蹤哪些實體頁框(4KB 為單位)目前是空的、可以拿去用。`kernel/src/mm/frame_allocator.rs` 用一個**堆疊式的自由串列(free list)**實作:

```rust
pub struct FrameAllocator {
    start_ppn: PhysPageNum,
    end_ppn: PhysPageNum,
    free_list: &'static mut [PhysPageNum],
    free_count: usize,
}
```

`init()`(`frame_allocator.rs:53-70`)在開機時把 `[start, end)` 這段實體位址範圍內的每個頁框號,依序塞進 `free_list`;`alloc()`(`frame_allocator.rs:75-81`)每次從堆疊頂端彈出一個頁框號,`dealloc()`(`frame_allocator.rs:87-93`)把釋放的頁框號推回堆疊頂端——都是 O(1) 操作。這個結構被包在一顆全域的 `SpinLock<FrameAllocator>`(`frame_allocator.rs:27`)裡,確保多核心(SMP)環境下配置/釋放不會互相踩到。

值得注意的是 `dealloc()` 有做**基本的重複釋放偵測**:如果 `free_count` 已經等於串列長度卻還有人要釋放頁框,代表某個頁框被釋放了兩次(double-free)——這種錯誤直接 `panic!`,比讓自由串列悄悄毀損、之後在隨機時間點配置出「同一塊實體記憶體被兩個用途同時持有」的災難好處理得多。

### RAII 的 `PageTable`:讓 Drop 幫你收尾

C xv6 的 `freewalk()`/`uvmfree()` 需要在每一個「頁表不再需要」的地方手動呼叫,忘了呼叫就是記憶體外洩(leak),呼叫兩次或呼叫時機不對就可能是 use-after-free。這份重寫把頁表包成一個帶 **RAII(Resource Acquisition Is Initialization)** 語意的 Rust 結構,定義在 `kernel/src/mm/page_table.rs:35-40`:

```rust
pub struct PageTable {
    root_ppn: PhysPageNum,
    walker: PageTableWalker,
    /// 這個 handle 是否擁有(owned)這棵樹,決定 Drop 時要不要釋放。
    owned: bool,
}
```

關鍵是這裡刻意分成「擁有(owning)」與「借用(borrowing)」兩種語意。`PageTable::new()`(`page_table.rs:58-64`)配置一個新的根頁表,回傳 `owned: true` 的實例;`PageTable::from_root()`(`page_table.rs:70-72`)只是包一層檢視,`owned: false`。真正做事的地方在 `Drop` 實作(`page_table.rs:159-177`):只有 `owned == true` 才會遞迴走訪並釋放整棵頁表樹(`free_walk`);而在遞迴釋放前,先把 trampoline 與 trapframe 這兩個「映射了但不屬於這個 process」的葉節點清掉(`unmap_nofree`,只清 PTE、不釋放實體頁)——trampoline 是所有 process 共用的核心程式碼頁,trapframe 是行程自己管理生命週期的頁,兩者都不該被 `free_walk` 意外收回頁框配置器。

實務效果是:一個 process 結束、它的 `PageTable` 物件被丟棄時,Rust 編譯器保證 `drop()` 一定會被呼叫恰好一次——不會忘記釋放,也不會釋放兩次。

### Kernel 與 User 位址空間

**Kernel 位址空間**由 `kvminit()`(`kernel/src/mm/page_table.rs:212-217`)在開機時建立一次,之後所有 hart 共用同一份。`map_kernel()`(`page_table.rs:219-272`)依序映射:UART(`0x10000000`)、VirtIO 磁碟控制器(`0x10001000`)、PLIC 中斷控制器,全部 identity mapping、`PTE_R|PTE_W`;kernel 程式碼段(`.text`,從 `KERNBASE = 0x80000000` 到 `etext`)用 `PTE_R|PTE_X`(可讀可執行,但不可寫——避免程式碼被意外或惡意竄改);從 `etext` 到 `PHYSTOP` 的整段實體記憶體都用 `PTE_R|PTE_W` identity mapping,涵蓋 kernel 的資料段、bss、開機堆疊、kernel heap,以及 frame allocator 手上那整包可配置的實體頁池。每個 process 的 kernel stack 下方都留了一個**未映射的 guard page**,kernel stack 溢位時會直接觸發 page fault 而不是悄悄踩壞相鄰記憶體。

**Trampoline** 是這裡的一個特例:它是一段固定放在虛擬位址空間**最頂端**的程式碼(`TRAMPOLINE = 0xFFFFFFFFFFFFF000`,定義於 `kernel/src/arch/asm.rs:186`),負責 user↔kernel 之間切換 `satp`、保存/還原暫存器的組合語言邏輯(`uservec`/`userret`)。它必須**同時映射進 kernel 頁表(`map_kernel`)和每一個 user 頁表(`uvmcreate`,`page_table.rs:281-289`)**,而且兩邊映射到同一段實體頁——原因是:當 CPU 切換 privilege mode 時 `satp` 也會跟著切換頁表,如果切換頁表的那段程式碼本身沒有在新舊兩份頁表裡都映射在同一個虛擬位址,CPU 執行到切換指令的下一行就會直接抓不到指令。這是 xv6 一貫的設計,C 版本與這份 Rust 版本邏輯完全對應。

**User 位址空間**的佈局(可參考 `kernel/src/proc/mod.rs:135-149` 的 `userinit`,`sys_exec` 走相同邏輯)由低到高依序是:從 `0` 開始的程式碼與資料段;對齊後接一個**未映射的 guard page**(user stack 若往下溢位,第一時間就是乾淨的 page fault);往上是 user stack(`USER_STACK_PAGES` 個頁,`R|W|U`);heap 再往更高位址由 `sbrk` 動態成長;位址空間最頂端固定映射 **trampoline**(`R|X`,無 `U`,user 不能直接讀寫)與 **trapframe**(`R|W`、無 `U`,存放進 trap 時保存的暫存器)。這個「guard page + stack 貼著程式碼、heap 往上長」的設計延續 C xv6 的簡化佈局,好處是 `sz`(process 目前用了多少虛擬位址)可以保持很小,`fork`/`exec`/`exit` 走訪整個位址空間的成本也就跟著低。

### fork 的 `uvmcopy`、exec 的位址空間重建、行程結束的 `uvmfree`

**`fork`** 需要讓子行程擁有一份和父行程一模一樣、但完全獨立的記憶體內容。實作在 `kernel/src/mm/page_table.rs:342-356` 的 `uvmcopy`:逐頁走訪來源頁表,對每一個有效的 PTE 配置一個新的實體頁框、把內容整頁複製過去、再用相同的 flags 映射進目的頁表。這是**深拷貝(deep copy)**——xv6 沒有實作寫時複製(copy-on-write),父子行程從 `fork` 那一刻起,記憶體就是徹底獨立的兩份實體頁。

**`exec`**(`sys_exec`,`kernel/src/proc/syscall.rs:474` 起)則是不同策略:不是修改現有位址空間,而是用 ELF loader(`kernel/src/elf.rs`)在**新的頁表**上建置全新的程式碼/資料段與 stack,成功後才把 process 的頁表指標換成新的一份、釋放舊頁表。這保證了「exec 到一半失敗」不會讓行程停留在半殘狀態。

**行程結束**時,`uvmfree()`(`page_table.rs:312-316`)逐頁解除 `[0, sz)` 範圍的映射並釋放實體頁框;但真正把**頁表結構本身**連根釋放的,是 `PageTable` 的 `Drop`——`uvmfree` 只清資料頁,頁表結構的回收交給 RAII 自動處理,兩者職責分開。要特別小心 `uvmdealloc`(縮小 heap,只釋放 `[new_sz, old_sz)`)與 `uvmfree`(整段釋放)的差異:絕不能拿 `uvmfree(pt, new_sz)` 去縮小 heap,否則會把還在跑的程式碼也一併解除映射。

### Demand Paging:第一次觸碰才配置

User stack 與 heap 並不是一開始就把所有虛擬頁對應好實體頁框,而是採用**延遲配置(demand paging)**:頁面先在虛擬位址空間裡「保留」,真正第一次被讀寫時才觸發 page fault,由 kernel 補上映射。觸發點在 `kernel/src/arch/trap.rs:246-261` 的 `usertrap()`:當 `scause` 是 `13`(load page fault)或 `15`(store page fault)時,呼叫 `handle_page_fault`(`kernel/src/mm/page_fault.rs:7-34`)。它的關鍵邏輯:

- 先做邊界檢查:位址是否超出 process 的合法大小(`sz`)、是否早已映射過。
- **一律 zero-fill**:frame allocator 給出來的實體頁不保證乾淨(可能殘留前一個使用者的資料),所以配置後立刻整頁清零,避免資訊外洩。
- **旗標固定是 `R|W|U`,不管這次 fault 是讀還是寫**。程式碼註解記錄了一段修過的 bug:舊邏輯曾依 `read` 參數決定要不要給 `PTE_W`,結果一次 store fault 反而拿到不可寫的頁面,導致同一個位址寫入指令無限重複 fault——典型的「條件邏輯寫反」案例。

### Kernel Heap:給 `alloc`、`Box`、`Vec` 用的記憶體

前面談的 frame allocator 管理「整頁(4KB)」為單位的實體記憶體,主要服務頁表、user 頁面。但 Rust 的 `alloc` 生態系(`Box`、`Vec`、`String`……)需要能配置任意大小的區塊,這就是 kernel heap 的工作。`kernel/src/mm/frame_allocator.rs:11-20` 宣告了一塊固定大小的靜態陣列作為 heap 儲存空間,並註冊成 Rust 的全域配置器:

```rust
const KERNEL_HEAP_SIZE: usize = 16 * 1024 * 1024; // 16MB
static mut HEAP: [u8; KERNEL_HEAP_SIZE] = [0; KERNEL_HEAP_SIZE];

#[global_allocator]
static ALLOCATOR: LockedHeap<32> = LockedHeap::<32>::empty();
```

用 `static mut` 而非不可變 `static` 是刻意的:不可變 `static` 會被連結器放進唯讀的 `.rodata`,分頁啟動後對它寫入會 page fault;`static mut` 則落在可寫的 `.bss`。有了這個全域配置器,`Box::new(...)`、`Vec` 等寫法才能在核心裡運作——這是 C xv6 完全沒有對應物的一層基礎設施。

---

## 陷阱與系統呼叫(Traps & System Calls)

### 什麼是「陷阱」:硬體強制觸發的函式呼叫

想像使用者程式正在跑,某一刻它想請作業系統幫忙(例如寫一個檔案),或是它不小心存取了一塊還沒配置的記憶體,又或者時鐘中斷剛好響了。這三種情況在 RISC-V 上有一個共同名字:**trap**。

Trap 可以類比成「由 CPU 硬體、而非程式本身發動的函式呼叫」——正常的函式呼叫是 `call foo`,程式自己決定何時跳走;trap 則是 CPU 在某個條件成立時,不管你正執行到哪一行,強制把 PC(程式計數器)切換到一個核心事先登記好的位址。RISC-V 把這個位址記在 `stvec` 這顆控制暫存器(CSR)裡。

Trap 有三種來源:

1. **系統呼叫(syscall)**:使用者程式主動執行 `ecall` 指令,相當於「我知道我在叫 CPU,請幫我進核心模式」。
2. **例外(exception)**:非預期的錯誤,例如存取未映射的記憶體(page fault)、除以零、非法指令。程式沒有主動要求,但 CPU 發現不對勁就跳出。
3. **中斷(interrupt)**:與正在執行的指令流無關的外部事件,例如時鐘中斷(讓核心可以定期搶回控制權做排程)、裝置中斷(如磁碟、UART 透過 PLIC 通知資料已就緒)。

CPU 用一個叫 `scause` 的 CSR 來標記「這次 trap 是哪一種」:最高位元是 1 代表是中斷,是 0 代表是例外/系統呼叫;`ecall` 對應的例外碼是 8(`kernel/src/arch/trap.rs:230`),缺頁例外是 13(load)/15(store)(`kernel/src/arch/trap.rs:246`)。

### 特權模式:為什麼需要 U-mode / S-mode 之分

RISC-V 定義了多種特權層級,xv6 只用到兩種:

- **U-mode(User mode)**:使用者程式執行的地方。這個模式下不能直接操作硬體(如頁表暫存器 `satp`)、不能任意存取核心記憶體。
- **S-mode(Supervisor mode)**:核心執行的地方,擁有完整硬體控制權。

這個區隔的目的是隔離:如果使用者程式可以任意存取記憶體或關閉中斷,一個惡意或有 bug 的程式就能弄垮整台機器,或偷看別的程序的資料。使用者程式想要「借用」核心的能力(寫檔、印字元、建立行程)時,只能透過受控的窄門——也就是系統呼叫——把控制權暫時交給 S-mode 的核心程式碼,核心做完事情再把控制權還回去。

`usertrap()` 一進來就檢查 `sstatus` 的 SPP(Supervisor Previous Privilege)位元,如果這個 trap 不是從 U-mode 來的就直接 panic(`kernel/src/arch/trap.rs:214-216`)——這是一個一致性檢查:「這支函式只該處理來自使用者的 trap」。

### 使用者 → 核心:一次 trap 的完整旅程

以系統呼叫為例,從使用者呼叫 `write()` 到核心真正處理它,發生了下面這串事:

```
[U-mode] 使用者程式執行 ecall (a7=系統呼叫編號, a0..a6=參數)
     │  硬體自動: 把 PC 存進 sepc, 把 scause 設成 8, 跳到 stvec 指向的位址
     ▼
[trampoline 頁, satp 仍是 user 頁表] uservec:
     │  把 32 個通用暫存器存進固定虛擬位址 TRAPFRAME
     │  從 trapframe 讀出核心要用的 kernel_satp / kernel_sp / kernel_trap
     │  切換 satp 成核心頁表 (sfence.vma)
     │  jr 到 usertrap
     ▼
[S-mode, 核心頁表/核心堆疊] usertrap() (Rust):
     │  讀 scause==8, epc 前進 4 (跳過 ecall 那條指令)
     │  呼叫 syscall() → proc_syscall() 分派實際處理函式
     │  結果寫回 trapframe.a0
     ▼
usertrapret() → userret (trampoline):
     │  切回使用者頁表 (satp), 從 TRAPFRAME 還原所有暫存器
     │  sret
     ▼
[U-mode] 使用者程式從 sepc(已 +4)之後繼續執行, a0 是系統呼叫傳回值
```

這個「先進 trampoline 存暫存器 → 換頁表 → 才進 Rust」的兩段式設計,是因為換頁表(`satp`)這個動作本身很危險:換完之後,原本執行到一半的那段程式碼所在的位址,在新頁表裡不一定還映射得到,PC 會直接失控。所以「切頁表」與「切頁表前後都要能執行」這件事,需要一段特殊的橋樑程式碼。

### trampoline:兩個房間之間共用的玄關

`uservec`/`userret` 這兩段組合語言被放在一個叫 **trampoline** 的特殊頁面裡(`kernel/src/arch/asm.S:40-54` 的註解說明得很清楚)。它的特性是:**同一段程式碼,同時被映射在每一個使用者頁表和核心頁表的同一個虛擬位址**(`TRAMPOLINE = 0xFFFFFFFFFFFFF000`,`kernel/src/arch/asm.rs:186`)。

可以把它想成兩個房間(使用者頁表所代表的位址空間、核心頁表所代表的位址空間)之間唯一共用的一道玄關:不管你站在哪個房間,推開這道門看到的裝潢(指令內容)都一樣,所以「換房間」(切換 `satp`)這個動作,可以安全地在玄關裡完成——因為玄關本身在切換前後都存在、都可執行,PC 不會斷在半空中。

TrapFrame 本身則放在玄關旁邊固定的虛擬位址 `TRAPFRAME = TRAMPOLINE - PGSIZE`(`kernel/src/arch/asm.rs:189`),但物理頁面其實是每個行程自己核心堆疊頂端的一塊記憶體(`kernel/src/proc/trapframe.rs:6-11`):`alloc_trapframe` 把它配置在 `kstack` 頂端往下 `TRAPFRAME_SIZE` 的位置。也就是說,「虛擬位址固定」但「背後對應的實體頁面每個行程不同」——這樣 `uservec` 才能用同一段程式碼(同一個立即數 `TRAPFRAME`)存取到「當下這個行程」自己的暫存器存檔區。

### TrapFrame:一份與組合語言簽了約的結構

`TrapFrame`(定義在 `kernel/src/arch/trap.rs:15-60`)是整個 trap 機制的核心資料結構,存了所有使用者暫存器,加上核心要用的幾個欄位:

```rust
#[repr(C, align(16))]
#[derive(Debug, Default, Clone, Copy)]
pub struct TrapFrame {
    pub kernel_satp: PhysPageNum,   // 核心頁表
    pub kernel_sp: usize,           // 核心堆疊指標
    pub kernel_trap: usize,         // usertrap 的位址
    pub epc: usize,                 // 使用者 PC (sepc)
    pub kernel_hartid: usize,       // 目前 CPU 核心 id
    pub ra: usize, pub sp: usize, /* ...使用者的 31 個通用暫存器... */
}
```

注意 `#[repr(C, align(16))]`:這代表 Rust 編譯器不能自由重排欄位順序,欄位在記憶體中的排列必須跟宣告順序一致,對齊到 16 位元組邊界。這不是隨便挑的屬性——`kernel/src/arch/asm.S` 裡的 `uservec`/`userret` 直接用寫死的位元組偏移量(如 `sd ra, 40(a0)`)存取記憶體。這代表 `trap.rs` 裡的欄位順序、型別大小,跟 `asm.S` 裡的偏移量,是**同一份契約的兩種語言版本**——改動欄位的順序、插入新欄位、改變欄位型別大小,若沒有同步修改 `asm.S` 對應的偏移量,就會讓組合語言讀到形狀不對的資料,而編譯器完全無法幫你檢查這種錯誤。這是 CLAUDE.md 中特別點名的高風險區域。

`sscratch` 在這裡扮演一個小技巧:`uservec` 入口時,唯一還可以自由使用的暫存器只有目前的 `a0`,但馬上又需要 `a0` 當作 TRAPFRAME 的基底位址去存其他暫存器,所以先把使用者的 `a0` 值暫存進 `sscratch`,等其餘暫存器都存完了,再把 `sscratch` 讀回來存進 trapframe 的 a0 欄位。這是「只有一個暫存器可以周轉,要怎麼安全存下所有暫存器」的經典解法。

### 系統呼叫如何分派

在 `usertrap()` 判定 `scause == 8`(是 ecall)之後(`kernel/src/arch/trap.rs:230-241`),流程是先讓 `epc += 4`,再開中斷,最後呼叫 `crate::syscall::syscall()`:

```rust
8 => { // environment call from user mode (syscall)
    if crate::proc::is_killed(p) { crate::proc::kexit(-1); }
    { let inner = p.lock(); unsafe { inner.trapframe.as_mut().unwrap().epc += 4; } }
    intr_on();
    crate::syscall::syscall();
}
```

`epc += 4` 很關鍵:`ecall` 觸發 trap 時,`sepc` 指向的是 `ecall` 這條指令本身;如果不把它往前移一條指令的長度(RISC-V 定長指令是 4 bytes),之後 `sret` 回使用者空間會不斷重複執行同一條 `ecall`,程式就卡死了。這裡先讓 `epc += 4`,再開中斷(`intr_on()`)——把中斷延後到確定 trapframe 的 epc 已修正之後才打開,是為了避免在還沒修正前被時鐘中斷打斷造成狀態不一致。

實際分派邏輯在 `proc_syscall()`(`kernel/src/proc/syscall.rs:56-96`):

```rust
pub fn proc_syscall() {
    let p = current_process();
    let tf_ptr = { let inner = p.lock(); inner.trapframe };
    let tf = unsafe { &mut *tf_ptr };
    let num = tf.a7;

    tf.a0 = match num {
        SYS_FORK => sys_fork() as usize,
        SYS_WRITE => sys_write(tf.a0, tf.a1, tf.a2) as usize,
        SYS_READ  => sys_read(tf.a0, tf.a1, tf.a2) as usize,
        // ...
        _ => { crate::printk!("unknown syscall {}\n", num); -1isize as usize }
    };
}
```

呼叫慣例(calling convention)跟使用者庫的 C ABI 保持一致:**編號放在 `a7`**,**參數依序放在 `a0..a6`**,**回傳值放回 `a0`**——這正好對應到 `TrapFrame` 裡的欄位,因為 `uservec` 早在進核心前就已經把使用者的 `a0`~`a7` 完整存進 trapframe 了,分派函式只是把它們當一般欄位讀取而已。系統呼叫編號跟 C 版 xv6 完全對齊(`kernel/src/proc/syscall.rs:32-54`,fork=1、exit=2……sync=22),確保由 C 版 mkfs/使用者程式編出來的 ELF 二進位不需重新編譯就能在這個核心上跑。

### 回到使用者:usertrapret 與 userret

系統呼叫、例外、可搶佔的中斷處理完之後,`usertrap()` 最終都會呼叫 `usertrapret(p)`(`kernel/src/arch/trap.rs:143-188`,回傳型別是 `!`,代表它永遠不會正常返回——只會透過 `sret` 跳走)。`usertrapret` 做的事,基本上是「把 uservec 下次進來時要用到的資訊準備好(核心頁表、核心堆疊、usertrap 位址都寫進 trapframe),把 `sstatus` 設成回到 U-mode、回去後開中斷,再交給 userret」。

這裡有個容易忽略但很關鍵的順序:`intr_off()` 必須在 `w_stvec(uservec_va)` **之前**執行,並且一路維持到 `sret` 為止。原因寫在註解裡(`kernel/src/arch/trap.rs:159-162`):一旦 `stvec` 指向 `uservec`,但目前 `satp` 還是核心頁表,這時若被中斷打斷,CPU 會跳去執行 `uservec`,而 `uservec` 第一件事就是存取 `TRAPFRAME` 這個虛擬位址——但 `TRAPFRAME` 只在**使用者**頁表裡有映射,核心頁表沒有,於是立刻 page fault,而 page fault 又是另一個 trap,又跳回 `uservec`……形成無窮的錯誤迴圈。

`userret`(`kernel/src/arch/asm.S`)則是上述流程的鏡像:先切回使用者頁表(`csrw satp, a0`),再從 TRAPFRAME 依序把 31 個暫存器讀回來(`a0` 留到最後才讀,因為它一直被拿來當基底位址用),最後執行 `sret`——這條指令會依照 `sstatus`/`sepc` 把特權層級降回 U-mode、把 PC 設回 `sepc`,使用者程式就從剛才 `ecall` 的下一行繼續跑。

### 核心自身的 trap:kerneltrap / kernelvec

核心程式碼執行中也可能被 trap 打斷——最常見的是時鐘中斷,要拿走 CPU 去排程另一個行程;或是不預期的核心 bug。這條路徑跟使用者 trap 完全分開,走 `kernelvec`,而不是 `uservec`。兩者最大的差異:`uservec` 只需要把暫存器存進**固定虛擬位址**的 TRAPFRAME(因為它接下來要換頁表);`kernelvec` 全程都待在核心頁表裡,不需要換頁表,所以它直接把 32 個通用暫存器整批壓進**目前的核心堆疊**,呼叫 `kerneltrap()`,回來後再逐一還原、`sret`。

`kerneltrap()`(`kernel/src/arch/trap.rs:359-391`)本身也做嚴格的假設檢查:必須是從 S-mode 進來、且進來時中斷必須是關著的,否則直接 panic——這兩個斷言反映了 xv6 對核心臨界區的紀律:核心程式碼在需要原子性的地方會主動關中斷,若這時還被打斷,代表某處鎖的紀律被破壞了,寧可 panic 也不要悄悄留下不一致狀態。`kerneltrap` 對中斷的處理很精簡:呼叫 `devintr()` 判斷這次是哪種中斷,如果是時鐘中斷,就呼叫 `yield_now()` 讓出 CPU 給排程器——這就是 xv6 搶佔式排程的實作核心:時鐘中斷本身沒有特別的排程演算法,它只是定期強迫目前正在跑的程式碼「被打斷一下」,讓排程器有機會介入決定接下來換誰跑。

### 中斷分派與 PLIC

`devintr()`(`kernel/src/arch/interrupt.rs:39-58`)是中斷的統一入口,靠 `scause` 的精確數值分辨兩種硬體中斷:

- **Supervisor external interrupt**:代表某個外部裝置透過 **PLIC**(Platform-Level Interrupt Controller)發出中斷。核心呼叫 `plic_claim()` 問 PLIC「這次是哪個裝置」(回傳一個 IRQ 編號),依編號分派給對應驅動(UART 是 IRQ 10、virtio 磁碟是 IRQ 1),處理完呼叫 `plic_complete(irq)` 通知 PLIC「這個中斷我處理完了」。
- **Timer interrupt**:推進全域 `tick` 計數並把下一次時鐘中斷的觸發時間 `stimecmp` 往後設一段固定間隔——這就是排程器賴以定期奪回控制權的心跳來源。

PLIC 是一個外接於 CPU 之外、獨立於 trap 機制的中斷收發總機:它負責「多個裝置共用一條中斷線」的仲裁與優先權排序;`scause` 只告訴核心「發生了一次外部中斷」,細節(哪個裝置)要另外問 PLIC 才知道——這與時鐘中斷不同,時鐘中斷的來源訊息完整地編碼在 `scause` 本身。(裝置驅動與 PLIC 的細節在第 5 章。)

### page fault:另一種例外處理路徑

`usertrap` 對 `scause` 為 13(load page fault)或 15(store page fault)的分支(`kernel/src/arch/trap.rs:246-261`),會取出目前行程的頁表,呼叫 `crate::mm::page_fault::handle_page_fault()` 嘗試修復(例如按需配置頁面);修復失敗才把行程標記為該終止。這代表 page fault 在 xv6 裡不必然是致命錯誤——它也是實作「懶惰配置記憶體(lazy allocation)」等機制的合法途徑(詳見第 1 章的 demand paging)。

### Rust 版與原始 C xv6 的差異

- **`unsafe`/組合語言被刻意收斂進 `arch/` 模組**:`usertrap`/`kerneltrap` 這些函式雖然用 `#[unsafe(no_mangle)] extern "C"` 對接組合語言,但函式本體內部的邏輯(如 `match scause { ... }` 分派)都是可被借用檢查器驗證的安全程式碼,只有真正碰記憶體位址、CSR 或裸指標的地方才用 `unsafe` 區塊。
- **型別化的 CSR 存取**:`kernel/src/arch/registers.rs` 用 `tock_registers` 的 `register_bitfields!` 巨集把 `SSTATUS`/`SCAUSE`/`SATP`/`PTE` 的欄位定義成有名字、有型別的位元欄位,而不是像 C 版一樣散落各處的裸魔術數字與手刻位移運算。
- **TrapFrame 用 `#[repr(C, align(16))]` 結構體取代 C 版的手寫欄位偏移巨集**:欄位仍然是「與組語的契約」,但至少在 Rust 這一側能用型別系統保證欄位大小、對齊一致。
- **RAII 化的事務邊界**:牽涉到檔案系統寫入的系統呼叫,用「建構時 `begin_op()`、`Drop` 時 `end_op()`」的守衛物件取代 C 版每個 `return` 前都要手動呼叫 `end_op()` 的模式,讓提早 return 不會漏掉交易提交。

### 小結

一次系統呼叫的完整生命週期是:硬體發現 `ecall` → 跳進共用玄關 `uservec`,把使用者暫存器存進這個行程專屬的 `TrapFrame` → 切換到核心頁表與核心堆疊,交給 Rust 的 `usertrap()` → 依 `scause` 判斷類型,系統呼叫的話讀 `TrapFrame.a7` 分派、讀 `a0..a6` 當參數、寫 `a0` 當回傳值 → `usertrapret()` 重新布置好下次 trap 要用的核心狀態 → 再次經過玄關 `userret`,切回使用者頁表、還原暫存器 → `sret` 回到使用者模式。整個機制的正確性,幾乎完全建立在「TrapFrame 的欄位佈局」與「trampoline 組合語言裡寫死的偏移量」能夠精準對應之上。

---

## Process 管理與排程(Processes & Scheduling)

### 什麼是 process

一個 process 是「正在執行的程式」——不只是一堆指令,而是指令加上執行它所需的一切狀態:一份獨立的位址空間(它以為自己獨佔整台機器的記憶體)、一組暫存器內容、開啟的檔案、目前工作目錄,以及一個「父行程是誰」的身分。作業系統核心用一個資料結構替每個 process 記錄這些狀態,傳統上稱為 **PCB(Process Control Block)**——process 不在 CPU 上執行的那段時間,它的一切都得先「存檔」到 PCB 裡;等排程器選中它,再從 PCB「讀檔」回 CPU。這一章要看的,就是這份 PCB 在 xv6-rust 裡長什麼樣子,以及排程器如何在多個 process 之間輪流讀存檔案。

### `Proc` / `ProcInner`:這份 PCB 存了什麼

`kernel/src/proc/process.rs:42-82` 定義了核心結構。這裡把 PCB 拆成了 `Proc`(外層、不可變)和 `ProcInner`(內層、被 `SpinLock` 包住的可變資料)兩層——這是原始 C xv6 沒有的分層。C 版的 `struct proc` 直接把所有欄位攤在一起,靠程式設計師自己記得「動用這個欄位前要先 `acquire(&p->lock)`」。Rust 版把這個約定寫進型別系統:`ProcInner` 裡的任何欄位,你**拿不到**除非先呼叫 `p.lock()` 拿到一個 `SpinLockGuard`。忘記上鎖在這裡不是「有可能忘記的紀律」,而是編譯不過。

```rust
pub struct ProcInner {
    pub state: ProcState,        // 狀態機目前的狀態
    pub killed: bool,
    pub xstate: i32,             // 退出碼(供父行程 wait 讀取)
    pub pid: usize,
    pub parent: Option<*mut Proc>,   // 父行程(用裸指標避開循環引用)
    pub kstack: usize,           // 核心堆疊基底位址
    pub sz: usize,               // 使用者位址空間大小
    pub pagetable: Option<PageTable>,
    pub trapframe: *mut TrapFrame,   // 使用者暫存器存檔(給組語 trampoline 用)
    pub context: Context,        // 核心層 context switch 用的暫存器快照
    pub ofile: [Option<Arc<File>>; NOFILE],  // 開啟檔案表
    pub cwd: Option<*const Inode>,   // 工作目錄
    pub name: [u8; 16],
}
```

`PROCS` 是一個固定大小(`NPROC = 64`)的靜態陣列(`kernel/src/proc/scheduler.rs:8`),xv6 沒有動態配置 process 表,配置一個新 process 就是在這個陣列裡找一個 `Unused` 的格子。`ofile` 用 `Arc<File>` 讓多個 process(例如 fork 之後的親子)能安全共享同一個檔案物件,靠原子引用計數決定何時真正關閉;`parent` 用原始指標而非 `Arc`,是為了避開循環引用(父指向子、子指向父會讓引用計數永遠降不到 0)。這是貫穿整個改寫的主題:盡量把運行期紀律換成編譯期或型別層級的保證。

### Process 狀態機

`ProcState`(`process.rs:16-30`)是一個列舉:`Unused`(空格子)、`Used`(剛配置、還在填初始欄位)、`Runnable`(就緒,排隊等 CPU)、`Running`(正在某顆核心上執行)、`Sleeping`(在某個「等待頻道」上睡著)、`Zombie`(已 `exit`,但父行程還沒 `wait` 回收退出碼)。

```
Unused --alloc_proc--> Used --userinit/fork--> Runnable
   ^                                              |
   |                                         scheduler 選中
   |                                              v
Unused <--free_proc-- Zombie <--exit-- Running <--sleep()--> Sleeping
                                          |                    ^
                                          +---- wakeup() ------+
                                          |
                                     yield_now()
                                          v
                                      Runnable
```

`Zombie` 是刻意設計的「屍體」狀態:process 的資源(位址空間等)可以在 `exit` 時就先清掉,但 PCB 格子本身要留著,好讓父行程能讀到退出碼。用 Rust `enum` 表示狀態機,相對 C 版用 `int` 常數的差異是:`match` 必須窮舉所有分支,加新狀態時編譯器會逼你檢查每一處 `match` 有沒有處理到——C 版一個忘了改的 `if` 分支不會有任何警告。

### Context switch:把桌面拍照存檔,再擺出下一個人的桌面

類比:想像一張共用辦公桌。A 正在用電腦工作,現在輪到 B 用同一張桌子了。A 得先把桌上目前的檔案、便利貼、螢幕視窗排列「拍照存檔」,清空桌面;B 上工時,先把「B 上次留下的擺設」照著上次拍的照片擺回去,才能接著剛才的進度做下去。這個「拍照存檔/擺回去」的動作就是 context switch,而那張照片,就是 `Context`。

`kernel/src/arch/trap.rs:83-98` 定義的 `Context` 只存了 `ra`(return address)、`sp`(stack pointer)和 `s0..s11` 這組 **callee-saved(被呼叫者負責保存)暫存器**。這是刻意的精簡:呼叫慣例本來就規定「呼叫者負責自己存 caller-saved 暫存器」,所以 context switch 只需要救 callee-saved 那組,其餘的早在呼叫 `swtch` 之前就被正常的函式呼叫機制存好了。

真正做「拍照/擺回去」動作的是組語函式 `swtch`(`kernel/src/arch/asm.S`):`swtch(old, new)` 先把現在的暫存器存進 `*old`,再把 `*new` 的值載進暫存器,最後 `ret`——而 `ret` 用的正是剛剛才載入的新 `ra`,所以這一 `ret` 實際上跳到了完全不同的一段程式碼,彷彿「原地換了一個人繼續執行」。

誰跟誰在互相切換?每顆 CPU(hart)都有自己的一份 `scheduler` context,存在 `Cpu` 結構(`kernel/src/proc/mod.rs:20-25`)裡;每個 process 也有自己的一份 context,存在 `ProcInner::context`。所以一次 `yield`/`sleep` 觸發的切換,永遠是「目前這個 process 的 context」與「這顆 CPU 的 scheduler context」互換——process 之間不會互相直接切換,一定要先繞回各自 CPU 的排程器迴圈,再由排程器切進下一個要跑的 process。

### Round-robin 排程器主迴圈

排程器 `scheduler()`(`kernel/src/proc/scheduler.rs:145-187`)的邏輯,可以類比成銀行叫號機:排程器不停繞圈,一格一格檢查 process 表,只要看到「號碼牌翻到『可服務』(`Runnable`)」的人,就叫他過去,等他辦完事(或暫時離開去等別的東西)再繼續叫下一位。這是最單純的 **round-robin(輪詢式)** 排程:沒有優先權、沒有時間配額計算,就是一輪一輪掃過陣列,遇到 `Runnable` 就切進去執行,等它自願讓出(或被計時器中斷打斷)就繼續掃下一格。這保證了公平性(沒有 process 會被永遠晾在後面),但也意味著 process 一多,平均等待時間會隨陣列大小線性增加。

有兩處細節特別值得放大看:

**其一,「在持有鎖的狀態下設定 `cpu.proc` 與切換」。** 程式碼在 `inner.state = ProcState::Running` 之後、切進 process 之前,並沒有釋放 `p.lock`——而是用 `core::mem::forget(inner)` 讓 `SpinLockGuard` 的 `Drop`(本來會自動解鎖)失效,把鎖「刻意」保持在鎖著的狀態,一路帶著它切換過去。這是仿照 C xv6 的紀律:鎖被鎖著這件事,順便讓中斷保持關閉貫穿整個 `swtch`(`SpinLock` 的實作是取鎖時關中斷、放鎖時才開回來)。process 那一側的 `sched()`/`forkret` 負責把這把「借來的鎖」放掉。

**其二,為什麼要「先設狀態,再放鎖」——避免 sleep/wakeup race。** 想像另一顆 CPU 上的 `wakeup()` 正在掃描 process 表,想確認某 process 是不是還在 `Sleeping`;如果排程器把鎖放開了才去改 `state`,`wakeup` 就有機會在中間那個空隙讀到一個「舊」狀態,誤判、或漏判——這種競態叫 **lost wakeup**。把「改狀態」和「持鎖」綁在一起,保證任何其他 CPU 想讀/改這個 process 的狀態,都得先排隊拿到同一把鎖,天然序列化掉了這個競態。

### 從 process 切回排程器:`sched()` / `yield_now()`

`yield_now()`(`scheduler.rs:189-199`)是「主動讓出 CPU」的入口:把自己狀態設回 `Runnable`、保持鎖著、呼叫 `sched()`;`sched()`(`scheduler.rs:209-234`)才是真正呼叫 `context_switch` 切回排程器 context 的地方。它有一串前置條件檢查(必須持有 `p.lock`、中斷必須是關的、狀態不能還是 `Running`),違反就直接 `panic!`——這些斷言把「呼叫這個函式前該滿足的隱性契約」變成明確可驗證的執行期檢查。

### `sleep`/`wakeup`:生產者—消費者式的等待

`sleep(chan, lock)`(`kernel/src/proc/mod.rs:249-271`)讓一個 process 在某個「頻道」(`chan`,通常就是某個核心資料結構的位址,當作識別碼用)上睡著,直到有人對同一個 `chan` 呼叫 `wakeup`。呼叫慣例是「呼叫前先鎖住某個條件鎖(`lock`),`sleep` 負責原子性地『釋放這把條件鎖 + 睡著』」——這正是課本上典型的生產者—消費者模式所需要的原子操作:如果「釋放條件鎖」和「真正睡著」中間有空隙,另一方完全可能在那個空隙裡剛好把資料準備好並呼叫 `wakeup`,而睡眠者卻還沒真的進入 `Sleeping` 狀態、於是永遠等不到那次喚醒(lost wakeup)。做法是:先拿到 `p.lock`,再釋放呼叫者的 `lock`,再把狀態改成 `Sleeping`,而且改狀態的整段都在 `p.lock` 保護之下,`wakeup` 也得靠同一把 `p.lock` 才能檢視/修改狀態——於是「釋放條件鎖」與「登記為睡眠中」對外表現成單一原子步驟。(鎖的底層細節見第 5 章。)

### 每顆 CPU 一份的 `Cpu` 結構、核心堆疊佈局

`Cpu`(`kernel/src/proc/mod.rs:20-25`)記錄每顆 hart 自己的排程狀態:目前在跑哪個 process、排程器的 context、中斷關閉巢狀計數(`noff`)和中斷曾否啟用(`intena`)。`mycpu()` 靠讀 `tp` 暫存器(每顆 hart 開機時各自寫入自己的編號)去索引這個陣列——這也是為什麼取值時中斷必須先關閉的理由:如果讀 `tp` 之後、讀 `cpu.proc` 之前被計時器打斷、process 被搬去別顆 hart 執行,讀到的就是舊 CPU 已經清空的欄位。

核心堆疊(kernel stack)是這份改寫版本一個值得注意的實作決定:每個 process 的 kstack 佈局是「保護頁(guard page,不映射)+ `KSTACK_PAGES` 頁可用堆疊」。**`KSTACK_PAGES = 1`**,和 C xv6 一致,只給每個核心堆疊一頁(4KiB)——這代表核心程式碼(包括 Rust 編譯出的、往往比 C 更深的堆疊框)必須很節制;而正因為下面緊接著一頁沒映射的保護頁,一旦真的溢位,MMU 會立刻丟出 store page fault 讓錯誤精準現形,而不是悄悄踩壞隔壁隨機配置出來的實體頁。

### `fork`:複製一份自己

`sys_fork()`(`kernel/src/proc/syscall.rs:98-189`)的流程,類比就是「影印一份自己的辦公桌」:

1. **先配置子行程再鎖父行程**:`alloc_proc()` 會依序掃描並鎖每個 process 槽位,若先鎖住父行程(它自己也是 `PROCS` 陣列裡的一格),`alloc_proc` 掃到它時就會嘗試對同一把鎖再上鎖而自我死鎖。所以鎖的順序被明確定成「子先於父」。
2. **複製使用者位址空間**:建立一份新頁表、映射子行程自己的 trapframe,最後 `uvmcopy(src, dst, sz)` 把父行程 `[0, sz)` 的每一頁實體記憶體真的複製一份給子行程(深拷貝,父子從此互不影響,詳見第 1 章)。
3. **複製 trapframe,子行程回傳 0**:

   ```rust
   let tf = unsafe { &mut *npinner.trapframe };
   *tf = unsafe { *pinner.trapframe };
   tf.a0 = 0; // fork returns 0 in child
   ```

   父行程呼叫 `fork()` 當下的整組使用者暫存器被整份複製過去,唯獨把子行程那份的 `a0`(放回傳值的暫存器)硬改成 0——這正是「`fork` 在父行程回傳子 pid、在子行程回傳 0」這個經典行為的實作方式:兩邊執行的其實是「同一段程式碼從同一個 `epc` 繼續往下跑」,差別只在那顆暫存器的值。
4. **複製檔案描述表與 cwd**:對每個開啟的 fd 呼叫 `filedup` 並包一層新的 `Arc`,`cwd` 也用 `idup` 多拿一份 inode 引用計數。
5. **設定子行程第一次執行的起點**:手動偽造一份 `Context`,把 `ra` 設成 `forkret` 的位址、`sp` 設成它自己核心堆疊的頂端。之後排程器第一次對這個子行程做 `swtch`,尾端的 `ret` 就會照著這顆偽造的 `ra` 直接跳進 `forkret`,它負責釋放排程器幫忙保持鎖住的 `p.lock`,再呼叫 `usertrapret()` 把 trapframe 裡的使用者狀態(含 `a0 = 0`)送回使用者模式,子行程從此才第一次「活起來」。

### `exec`:換一顆全新的腦袋

`sys_exec()`(`kernel/src/proc/syscall.rs:474` 起)不是新建 process,而是讓**同一個** process 原地脫胎換骨——pid 不變、父子關係不變,但整個使用者位址空間被整組換掉:先從使用者記憶體讀出 `path` 和 `argv`,用 ELF loader 把新程式載入到一份**全新**的頁表(而非原地覆寫舊頁表,這樣萬一載入失敗舊映像完好無損),排好使用者堆疊與 `argv`,**確定新映像完全就緒後**才鎖住 process、把頁表換成新的、釋放舊頁表、把 trapframe 的 `epc`/`sp` 指向新程式進入點。`fd` 表完全沒動——這正是 `exec` 語意的一部分:換程式碼、換位址空間,但保留已開啟的檔案描述符,`sh` 靠這個機制實作 I/O 重新導向。

### `exit` / `wait`:化為 Zombie,等父行程收屍

`sys_exit(code)`(`kernel/src/proc/syscall.rs:191-232`)永不回傳:先關掉所有開啟的 fd、釋放 `cwd` 的 inode 引用(包在檔案系統交易裡,因為 `iput` 可能真的要寫回磁碟)、把自己所有還活著的子行程 `reparent` 給 init(對應 Unix 的經典規則:孤兒子行程要過繼給 init,讓 init 永不停止的 `wait()` 迴圈負責回收它們)、`wakeup` 可能睡在 `sys_wait` 裡的父行程,最後才在 `p.lock` 保護下把 `xstate` 設成退出碼、`state` 設成 `Zombie`,然後 `sched()` 切走,再也不會回來。

而 `sys_wait(addr)`(`kernel/src/proc/syscall.rs:234-296`)是父行程這邊的迴圈:掃描 `PROCS` 找出 `parent == 自己` 的子行程,一旦找到 `Zombie` 的,就把退出碼複製回使用者傳入的 `addr`,呼叫 `free_proc()` 真正釋放它的位址空間、trapframe,把槽位標回 `Unused`;若暫時沒有已死的子行程但確實有活著的,就 `sleep` 等下一次被叫醒。整個 `fork → exec → exit → wait` 串起來,就是這份程式碼裡「process 從生到死」的完整旅程。

---

## 檔案系統(File System)

xv6 的檔案系統是經典的七層架構,從下到上分別是:磁碟(disk)→ buffer cache → logging → inode → directory → pathname → file descriptor。每一層只依賴下一層提供的抽象,不越級呼叫。這一章就照這個順序,由下而上把每一層的職責與 Rust 實作講清楚——特別是 logging 那一層,那是整個檔案系統「保證斷電不壞資料」的核心機制。

### 磁碟佈局:一顆硬碟怎麼切成檔案系統

xv6 把磁碟切成連續的區塊(block),每個 block 是 1024 bytes,對應底層 virtio 磁碟的兩個 512-byte sector(`kernel/src/fs/buf.rs:22`)。磁碟由前到後依序是:**boot block**(block 0,開機用)、**super block**(block 1,描述整個檔案系統的「目錄」)、**log 區**(write-ahead log 使用)、**inode 區**(存放所有 on-disk inode)、**bitmap 區**(記錄每個 data block 是否被使用)、**data 區**(實際檔案與目錄內容)。

super block 的欄位定義在 `kernel/src/fs/log.rs:313-324`:

```rust
#[repr(C)]
#[derive(Copy, Clone)]
pub struct SuperBlock {
    pub magic: u32,
    pub size: u32,       // 檔案系統總大小(block 數)
    pub nblocks: u32,    // data block 數
    pub ninodes: u32,    // inode 數
    pub nlog: u32,       // log 區塊數
    pub logstart: u32,   // log 起始 block 號
    pub inodestart: u32, // inode 區起始 block 號
    pub bmapstart: u32,  // bitmap 區起始 block 號
}
```

開機時 `fsinit()`(`kernel/src/fs/mod.rs:20-27`)會依序:初始化 buffer cache、初始化 inode cache、讀取 superblock、用它初始化 log 子系統,最後跑一次崩潰復原。這個順序本身就透露了層與層的依賴關係——log 需要先知道 super block 才能定位自己的區塊,而 log 又必須在任何檔案操作之前準備好,才能保證後續寫入都有交易保護。

### Buffer cache:磁碟的便利貼暫存

磁碟很慢,同一個 block 常常在短時間內被讀寫好幾次(例如同一個目錄的多次查找)。Buffer cache 就是把最近用過的磁碟 block 留在記憶體裡的一張「便利貼牆」——每張便利貼記著「這是哪個裝置的第幾個 block」,內容是這個 block 的 1024 bytes 拷貝。要用某個 block 時先在牆上找,找到就直接用,免去一次磁碟 I/O;找不到就騰一張最久沒用的便利貼出來,去磁碟讀回內容。

`kernel/src/fs/buf.rs` 裡的 `BufCache` 維護 30 個 `Buf` 槽位(`NBUF = 30`,取 `MAXOPBLOCKS * 3`)加一條雙向鏈結串列做 LRU。核心函式:`bget(dev, blockno)` 先線性掃描是否已快取,命中就把 refcnt+1、移到 LRU 最前端;沒命中就從 LRU 尾端(最久沒用)找一個 refcnt==0 的槽位回收。若全部 30 個槽位都在使用中(refcnt>0),代表有洩漏(忘記 `brelse`),直接 `panic`——這不是暫時性擁擠,是不變式被違反。`bread` 呼叫 `bget` 拿到 buffer,若尚未 `valid` 就去磁碟讀;`bwrite` 把 buffer 內容寫回磁碟;`brelse` 把 refcnt-1,歸零時移回 LRU 尾端待回收。

**兩層鎖的設計**:注意 `Buf` 結構(`buf.rs:35-40`)把欄位分成兩組——`dev`/`blockno`/`refcnt` 是「身分」欄位,只受外層的 `BUF_CACHE` **spinlock** 保護;`valid`/`data` 是「內容」,包在各自 buffer 的 **sleeplock** 裡。為什麼要拆成兩層?因為 `bget` 在掃描、回收 buffer 時必須拿 spinlock,而 spinlock 持有期間中斷是關閉的,絕對不能在這段時間內睡眠。但讀寫磁碟內容可能要等 I/O 完成,勢必得睡眠。拆成兩層之後,`bget` 只碰 spinlock、絕不睡眠;真正要讀寫內容時,先放掉 spinlock 再去拿 sleeplock。底層真正的磁碟存取則透過 virtio 驅動完成(第 5 章)。

### Logging:先寫草稿,再一次謄正

**為什麼不能直接寫磁碟?** 一次檔案系統操作(例如建立檔案)往往要改好幾個磁碟 block:配置一個新 inode、更新目錄的資料 block、更新 bitmap……如果逐一直接寫到各自的「正式位置」(home location),一旦寫到一半斷電,磁碟上就會停在一個不上不下的狀態——比如 inode 已標記為使用中,但目錄項目還沒寫進去。這種「部分寫入」正是檔案系統崩潰後常見的損毀來源。

xv6 的解法是 **write-ahead logging(WAL)**,可以類比成「先把所有要改的內容寫在一份草稿(log)上,草稿完整寫好之後,才一次性宣告『這份草稿生效了』,然後才真的謄到正式位置」。只要中途斷電,要嘛草稿還沒宣告生效(整個操作等於沒發生,乾淨地作廢),要嘛草稿已經生效(重開機時把草稿重新謄一次正式位置即可)——不會停在半謄的狀態,因為謄正這一步是冪等(重放兩次結果相同)且開機時會自動重放。

**Transaction 的邊界:`begin_op`/`end_op`。** 每個檔案系統系統呼叫(如 `write`、`mkdir`)都被 `begin_op()`(`kernel/src/fs/log.rs:101-119`)和 `end_op()` 包起來。`begin_op` 會檢查:目前沒有 commit 正在進行,而且 log 剩餘空間足夠容納這個操作最壞情況下的 `MAXOPBLOCKS`(=10)個 block——不夠就睡眠等待。這個保守估計保證了同時執行的多個操作,合計起來絕不會超過 log 容量(`LOGSIZE`=30)。

**Log absorption:`log_write`。** 操作過程中,不直接呼叫 `bwrite` 把改動寫到磁碟,而是呼叫 `log_write(&buf)`(`log.rs:156-181`)登記「這個 block 屬於本次 transaction」。如果同一個 block 在同一次 transaction 裡被改了好幾次,`log_write` 會找到既有的紀錄槽位重複使用,而不是佔用第二個槽位——這叫 log absorption,讓一次 transaction 花的 log 空間跟「碰過幾個不同 block」成正比,而不是跟「寫了幾次」成正比。第一次登記某個 block 時,還會呼叫 `bpin` 把它釘在 buffer cache 裡(避免在 commit 完成前被 LRU 回收掉)。

**Commit 的四個步驟。** 當最後一個 outstanding 的操作呼叫 `end_op` 時,就觸發 `commit()`(`log.rs:184-206`):

1. **`write_log`**:把快取裡所有已登記(pinned)的 block,依序複製到磁碟上 log 區的連續位置。這一步只是把資料搬到 log 區,還沒動到任何 block 的「正式位置」。
2. **`write_head`**:把記著「log 裡有幾個 block、各自的正式位置是哪裡」的 `LogHeader` 寫到磁碟上 log 區的第一個 block。**這一步是整個 transaction 的 commit point**——一旦這個 header(`n > 0`)寫進磁碟,這個 transaction 就算是「已發生」,即使接下來馬上斷電也一樣。
3. **`install_trans`**:把 log 區裡的每個 block 讀出來,寫到它真正的正式位置(home location),然後 `bunpin` 解除釘選。
4. 清空:把記憶體裡的 `lh.n` 歸零,再寫一次空的(`n=0`)header 到磁碟,代表這次 transaction 已經完全結束、log 可以重複使用了。

**開機復原:`recover_from_log`。** 崩潰後重開機,`fsinit` 會呼叫 `recover_from_log()`(`log.rs:295-310`):讀出磁碟上的 header,如果 `n > 0`,代表上次斷電發生在「commit point 已寫入、但 home location 還沒裝完」之間——直接重新跑一次 `install_trans` 把 log 內容謄到正式位置,再清空 header。如果 `n == 0`,代表那次操作等於沒發生,什麼都不用做。這正是為什麼 commit point 只有「寫 header」這一步:寫之前所有動作都可安全捨棄,寫之後所有動作都可安全重放。

用一張簡單的時間軸看一次完整 commit:

```
write_log (log 區已有完整草稿)
      │
      ▼
write_head(n>0)  ← commit point:斷電後重開機必定重放
      │
      ▼
install_trans (謄到正式位置)
      │
      ▼
write_head(n=0)  ← 這次 transaction 結束,log 可重用
```

模組文件也點出了一個容易忽略的鎖規則(`log.rs:14-21`):`commit()` 本身以及它呼叫的 `write_log`/`install_trans`/`write_head` 都會用到 `bread`/`bwrite`,而這些函式最終要拿 sleeplock、可能睡眠,所以 **commit 執行時完全不持有 `LOG` 這把 spinlock**。安全性靠的是 `committing` 這個旗標——commit 期間所有其他要開新 transaction 的呼叫者都會在 `begin_op` 卡住睡眠,所以同一時刻只有一個 process 在碰 log 的磁碟結構。

### Inode:磁碟上的 vs 記憶體裡的

`DiskInode`(`kernel/src/fs/inode.rs:38-51`)是磁碟上實際的 64-byte 結構,對映到 C xv6 的 `struct dinode` 以保持磁碟相容;`Inode`(`inode.rs:97-113`)則是記憶體裡的活躍句柄,額外多了 `refcnt: AtomicUsize` 這個記憶體內參照計數,以及兩把鎖:`spinlock` 保護 `typ`/`size`/`addrs` 等中繼資料的快速存取,`lock: SleepLock<()>` 則是「這個 inode 目前正被誰獨佔操作」的邏輯鎖(`ilock`/`iunlock` 的對應)。

**Inode cache 的生命週期。** `iget` 在 256 個槽位(`NINODE`)的全域快取裡找到或回收一個槽位、把 refcnt+1。`iput` 則相反:refcnt-1,若歸零且磁碟上的 `nlink==0`(沒有目錄項目再指向它),就代表這個檔案已經被 unlink 到沒有名字也沒有開啟者了,於是釋放所有資料 block、把 on-disk inode 的 `typ` 寫回 0。程式碼裡的註解特別點出一個真實踩過的坑:回收 in-memory 槽位時要挑 `refcnt==0` 的槽位,而不是只挑 `typ==None` 的——因為一個曾經開啟又關閉、但仍有其他名字連結著的檔案,`typ` 不會被清掉,只有 `refcnt` 會降到 0;如果只認 `typ==None`,快取槽位會被永久佔用而洩漏。**refcnt 用 `AtomicUsize`** 是 Rust 版相對 C 版的具體差異:讓「引用計數變動」和「快取槽位掃描/回收」是兩件可以分開推理的事。

**bmap:direct block + indirect block,像「目錄的目錄」。** 一個檔案的內容分散在許多 block 裡,`addrs` 陣列(`NADDR = NDIRECT + 2 = 13` 格)記著怎麼找到它們。`bmap`(`inode.rs:285-319`)把邏輯上的第 `bn` 個檔案 block 換算成實際磁碟 block 號:`bn < NDIRECT`(11 個)直接查 `addrs[bn]`;再往後 `NINDIRECT`(=256)個落在 singly-indirect 範圍,`addrs[NDIRECT]` 指向一個「block 位址表」,裡面 256 個 entry 各指一個資料 block——這一層可以想成「檔案的目錄」;再更後面是 doubly-indirect,`addrs[NDIRECT+1]` 指向一個一級索引 block,一級索引裡的每個 entry 又各指向一個二級索引 block——也就是「目錄的目錄的目錄」,兩層間接讓單一 inode 能定址到約 64 MiB。

特別值得注意的是,`read`/`write`/`truncate` 都是先把 `addrs` 從 spinlock 保護的區域拷貝出來、在拷貝上操作(可能新配置了 block)、操作完成後才重新拿 spinlock 把更新後的 `addrs` 寫回去並持久化(`iupdate`)。原因是 `bmap` 內部要做 buffer I/O,而這可能睡眠、進而去搶別的鎖,絕不能在持有 `InodeInner` 這把 spinlock 期間發生。

### Directory:目錄其實是一種特殊檔案

xv6 沒有給目錄設計獨立的資料結構——目錄的 `typ` 是 `I_DIR`,但它的內容就是一連串固定大小的 `Dirent` 記錄(`inum` + `name`),用跟普通檔案完全一樣的 `read`/`write` 讀寫。`inum == 0` 代表這個槽位是空的。`dirlookup` 線性掃描目錄內容,逐一比對 `name`,找到就用該 `inum` 呼叫 `iget` 回傳對應的 inode;`dirlink` 先確認名字不存在,再找第一個空槽位寫入(掃到底都沒空位就在目錄末端 append)。

### Pathname 解析:一段一段查

`namei`/`nameiparent` 都建立在 `namex`(`inode.rs:769-827`)這個共用實作上。邏輯很直接:路徑以 `/` 開頭就從根目錄出發,否則從目前 process 的工作目錄(`cwd`)出發;把路徑用 `/` 切成一串 component;逐一處理每個 component:鎖住目前的目錄 inode、確認它真的是目錄、`dirlookup` 找下一層、解鎖並釋放上一層、把「目前的 inode」換成下一層。`nameiparent`(給 `create`/`link`/`unlink` 用)在最後一步之前停下,回傳父目錄和最後一段名字;`namei` 則一路走到底,回傳目標本身的 inode。

### File Descriptor 層:把檔案、pipe、裝置統一起來

最上層的 `File`(`kernel/src/fs/file.rs:15-121`)是一個薄的、`Arc<SpinLock<FileInner>>` 包起來的物件,`FileInner` 裡的 `typ: FileType`(`None`/`Pipe`/`Inode`/`Device`)決定這個檔案描述子背後到底是什麼。`fileread`/`filewrite` 照 `typ` 分派:`Pipe` 交給 pipe 的讀寫(第 5 章);`Inode` 走 `inode.lock()` → `inode.read`/`write` → `inode.unlock()`,並在同一段鎖的臨界區內讀取、更新共用的檔案偏移量 `off`——這保證了兩個透過 `fork` 共享同一個 fd 的 process,讀寫時會依序、看得到彼此的偏移量推進。

一個值得留意的鎖序細節:`fileread`/`filewrite` 都先把需要的欄位從 `FileInner` 的 spinlock 臨界區裡拷貝出來、立刻釋放鎖,才去做真正可能睡眠的 I/O——如果 lock 一路持有到 I/O 結束,而 I/O 又睡眠了,另一個共享同一個 `File` 的 process 呼叫 `fileclose` 想要減少引用計數就會永遠拿不到這把 spinlock,造成死結。這跟前面 buffer cache「識別/內容分兩層鎖」是同一個設計哲學的不同應用:凡是可能睡眠的操作,絕不能在持有 spinlock 時進行。`filewrite` 對 `Inode` 型別還會把大寫入拆成多個小 transaction,確保每次 `begin_op`/`end_op` 之間寫入的 block 數不超過 `MAXOPBLOCKS`。

### Rust 版 vs 原始 C xv6 的幾個差異

- **型別化取代裸整數**:`InodeType`、`FileType` 用 enum 取代 C 版本裡 `#define`/裸 `short`,配置錯誤(例如把裝置檔當目錄用)在明確的 `match` 分支就會現形。
- **RAII 鎖**:C xv6 手動配對 `acquire`/`release`,Rust 版本的 guard 靠 `Drop` 自動釋放;唯一的例外是 `Inode::lock()` 刻意用 `core::mem::forget` 讓鎖跨越多個函式呼叫持續持有,對應 C 版本 `ilock`/`iunlock` 手動配對的用法,並在文件註解裡標明「呼叫者必須自己配對呼叫 `unlock()`」。
- **原子引用計數**:inode 的 `refcnt` 用 `AtomicUsize` 取代 C 版被大鎖整體保護的裸 `int ref`。
- **不變式用註解與 panic 表達得更明確**:例如 `bget` 找不到空閒 buffer 時直接 panic 並在註解裡說明這代表洩漏而非暫時擁擠——把 C 版本裡靠開發者共識維持的隱性規則,搬到程式碼旁邊變成可讀的顯性文件。

---

## 同步、IPC 與裝置驅動(Synchronization, IPC & Drivers)

### 同步(Synchronization)

#### 為什麼需要鎖

xv6 是一個支援多核心(SMP)的作業系統:多個 CPU 核心可能同時執行核心程式碼,並存取同一份共享資料——例如行程表、緩衝區快取、pipe 緩衝區。如果兩個核心同時對同一份資料做「讀出、修改、寫回」,就可能發生 **race condition**:最終結果取決於執行時機的巧合,而不是程式邏輯。任何一段「必須讓同一時間只有一個執行緒進入」的程式碼稱為 **critical section(臨界區)**,而鎖(lock)就是用來保護臨界區的機制。即使是單核心,中斷也可能在任何指令之間插進來執行另一段程式碼,一樣可能打斷臨界區——所以鎖的設計必須同時考慮「多核心並行」與「中斷插入」兩種威脅。

#### spinlock:忙等到能進去為止

`SpinLock`(`kernel/src/sync/spinlock.rs:30-113`)是核心中最基本的鎖。它的核心概念很像「不斷敲門直到有人開門」:`acquire()` 用一個原子的 compare-and-swap(`AtomicBool::swap`)不斷嘗試把鎖從「未持有」翻成「持有」,翻不成功就在原地 `spin_loop()` 繼續轉,不會讓出 CPU。

**為什麼取得 spinlock 時要先關中斷?** 這是 `push_off()` 在做的事。設想一種情境:核心執行緒 A 正持有某個 spinlock,這時發生一次中斷,中斷處理常式剛好也想拿同一把鎖——A 被中斷卡住、中斷處理常式又忙等 A 手上的鎖,兩者永遠等不到彼此,造成死結(deadlock)。解法就是「拿鎖之前先把本核心的中斷關掉」,讓臨界區內不可能被中斷插隊,鎖釋放後才恢復中斷。

`push_off`/`pop_off`(`spinlock.rs:146-169`)用一個巢狀計數器(`cpu.noff`)搭配「進入前中斷狀態」的快照(`cpu.intena`)處理**巢狀**鎖的情況:多把鎖可以疊著拿,只有當最外層的鎖釋放(`noff` 歸零)時,才把中斷還原成最初的狀態,而不是無腦地重新打開中斷:

```rust
pub fn push_off() {
    let intr = intr_get();
    intr_off();
    let cpu = crate::proc::mycpu();
    cpu.noff += 1;
    if cpu.noff == 1 { cpu.intena = intr; }
}

pub fn pop_off() {
    let cpu = crate::proc::mycpu();
    if cpu.noff == 0 { panic!("pop_off: noff == 0"); }
    cpu.noff -= 1;
    if cpu.noff == 0 && cpu.intena { intr_on(); }
}
```

`holding()` 則用來判斷「目前這顆 CPU 是不是這把鎖的持有者」,主要供除錯用的斷言使用(鎖內部用 hart id 以 `hartid + 1` 編碼記錄擁有者,讓 `0` 明確代表「沒人持有」)。

#### Rust 的 guard-based RAII 鎖

C 版 xv6 的鎖是「手動配對」的:`acquire(&lock)` 之後,必須記得在每一條可能的離開路徑上呼叫 `release(&lock)`,忘記解鎖或多解一次都是常見錯誤來源。Rust 版把「鎖」和「被保護的資料」直接綁在一起(`SpinLock<T>` 內部用 `UnsafeCell<T>` 包住資料),`acquire()` 傳回一個 `SpinLockGuard<'a, T>`。這個 guard 實作了 `Deref`/`DerefMut`,所以用起來就像直接操作資料本身;而當 guard 離開作用域被 drop 時,`Drop` 實作會自動釋放鎖:

```rust
impl<'a, T> Drop for SpinLockGuard<'a, T> {
    fn drop(&mut self) {
        unsafe { *self.lock.cpu.get() = 0; }
        self.lock.locked.store(false, Ordering::Release);
        pop_off();
    }
}
```

這樣一來,「忘記解鎖」在正常路徑上幾乎不可能發生——連函式中途 `return`、`?` 提早返回,只要 guard 的作用域結束就會自動解鎖。程式碼中偶爾會看到刻意用 `core::mem::forget(guard)` 把 guard「作廢」(例如 `sleep()` 或 pipe 裡),這是有意識地把「誰來解鎖」的責任轉移給後續程式碼,而不是真的漏解鎖——這類地方都會有註解說明。

#### spinlock vs sleeplock

spinlock 是「忙等」策略:CPU 原地空轉直到拿到鎖,好處是延遲低、實作簡單,但持鎖期間**不能睡眠**(因為中斷已關閉,若真的睡著,整顆 CPU 就沒人能把它叫醒),因此只適合保護**很短**的臨界區。`SleepLock`(`kernel/src/sync/sleeplock.rs`)則是反過來:「留言後去睡覺,等被叫醒」。如果鎖被別人拿走,呼叫者不會忙等,而是把自己標記成 `Sleeping` 並讓出 CPU 給排程器,直到持鎖者釋放鎖時被 `wakeup` 叫醒。適用場景是**可能長時間持有、且持鎖期間可能需要睡眠**的臨界區——最典型的例子就是第 4 章磁碟緩衝區的「內容」鎖:讀取內容可能要真的去等磁碟 I/O 完成,這段時間絕不能讓其他核心也忙等空轉。注意 `SleepLock` 內部仍然靠一把小小的 `guard_lock: SpinLock<()>` 來保護「是否已鎖」這個旗標本身——sleeplock 只是把「等待方式」換成睡眠,底層短暫的旗標存取仍然需要 spinlock 互斥。

#### sleep/wakeup:條件等待

`sleep()`/`wakeup()` 讓行程可以「在某個條件成立之前先睡著,條件滿足時被叫醒」,不必忙等(真正在跑的實作在 `kernel/src/proc/mod.rs:249-298`)。呼叫慣例是 `sleep(chan, lock)`:呼叫者必須已經持有某把「條件鎖」`lock`(保護著要等待的條件),`sleep` 會**原子地**釋放這把鎖並讓行程睡著。

**為什麼一定要「先把狀態設成 Sleeping,再放掉條件鎖」?** 這是避免 **lost-wakeup race** 的關鍵。想像相反的順序:如果先放掉條件鎖、才把自己標成 Sleeping,那麼在這兩步之間,另一顆核心可能已經改變了條件並呼叫 `wakeup`——但此時被等待的行程狀態還是舊的(不是 `Sleeping`),`wakeup` 掃描行程表時完全看不到它,這次喚醒就「憑空消失」了,行程會永遠睡下去。`sleep()` 的作法是:先拿到自己的 `p.lock`,在**同一把鎖保護下**把狀態設成 `Sleeping`,然後才釋放外部的條件鎖——`wakeup()` 要修改行程狀態同樣得先拿到 `p.lock`,兩者互斥,就不會有中間的空窗期。`chan` 只是一個當作「識別碼」使用的數字,通常直接拿某個共享資料結構的位址來當(例如 pipe 用 `Arc::as_ptr` 取得共享緩衝區的位址),確保同一份資料的所有等待者都在同一個頻道上碰頭。

### 行程間通訊(IPC):pipe

#### pipe 是什麼

Shell 裡的 `|`(例如 `ls | grep foo`)背後就是 pipe:核心建立一段緩衝區,一端只能寫、一端只能讀,兩個行程分別拿到寫端與讀端的檔案描述符,資料從寫端流向讀端。可以把 pipe 想成一條**輸送帶**:生產者(寫端)把東西放上輸送帶前端,消費者(讀端)從輸送帶尾端取貨;輸送帶滿了生產者要停手,輸送帶空了消費者要等待。

#### 環形緩衝與生產者/消費者協調

`Pipe`(`kernel/src/fs/pipe.rs`)內部是一個 `Arc<SpinLock<PipeInner>>`,`PipeInner` 用 `nread`/`nwrite` 兩個游標搭配固定大小(`PIPE_SIZE = 512`)的緩衝來實作。用 `Arc` 包起來很重要:讀端 `File` 與寫端 `File` 都持有同一個 `Pipe` 的 clone,`Clone` 只是複製 `Arc` 指標、共享同一份底層資料——否則兩端各自寫入自己的副本,資料永遠傳不過去。

讀取的邏輯:如果緩衝是空的(`nread == nwrite`)而且寫端還開著,就把自己交給 `sleep()` 睡著,等寫端寫入東西後被 `wakeup` 叫醒再重新檢查:

```rust
while inner.nread == inner.nwrite && inner.write_open {
    let chan = self.chan();
    core::mem::forget(inner);
    sleep(chan, &*self.inner);
    inner = self.inner();
}
```

醒來後重新 `acquire()` 拿到新的 guard,再回到迴圈開頭檢查條件(**必須用 `while` 而非 `if`**——被叫醒不代表條件一定成立,可能是別的等待者搶先消費了資料,所以要重新檢查)。真的讀到資料後,如果緩衝被讀空就把游標歸零,並呼叫 `wakeup` 通知可能在等空間的寫端。寫入的邏輯是對稱的:緩衝滿了就先 `wakeup` 讀端再讓自己睡著等空間;如果讀端已經全部關閉,寫入直接中止,對應到 Unix 語意上的「broken pipe」。`read_close()`/`write_close()` 在某一端關閉時,把對應旗標關掉並 `wakeup` 對面的等待者——確保「另一端已經沒人了」這個事實能即時傳達,不會讓對方永遠卡在 `sleep` 裡等一個再也不會發生的事件。

### 裝置驅動(Drivers)

#### MMIO 是什麼

裝置驅動是核心中負責跟實體硬體溝通的一層。QEMU 的 `virt` 機器模型把每個裝置的控制介面對應到一段固定的實體位址範圍(**memory-mapped I/O**,MMIO):對這段位址做「讀」或「寫」,實際上不是在存取記憶體,而是在跟裝置的暫存器互動,驅動只要用 `read_volatile`/`write_volatile` 操作這些位址即可。`volatile` 很關鍵——它告訴編譯器「這個存取有副作用,不可以被最佳化掉或重排」,因為一般記憶體存取的最佳化假設(例如「連續兩次寫同一位址,可以只留最後一次」)在裝置暫存器上完全不成立。

#### UART console 驅動

UART(通用非同步收發器,`kernel/src/drivers/uart.rs`)對應到 `0x10000000`,是最簡單的字元裝置:`uart_putc` 忙等 Line Status Register 的 TX-empty 位元,直到傳輸暫存器空出來才把字元寫進去;`uart_getc` 是非阻塞的,檢查 RX-ready 位元決定要不要讀出一個字元。核心的 `printk`(用於 debug 輸出與 panic 訊息)直接逐字元呼叫 `uart_putc`,並用 `intr_off()`/`intr_on()` 包住整個輸出過程,避免輸出中途被中斷打斷。真正給使用者程式用的 console 讀寫(`kernel/src/drivers/console.rs`)則多包一層 `CONSOLE_LOCK` 做行內編輯與 `\n`→`\r\n` 轉換;`console_read` 目前的實作是輪詢 `uart_getc()`,拿不到字元就呼叫 `yield_now()` 讓出 CPU 而不是真的睡眠等中斷喚醒——這跟原始 C xv6 用中斷驅動 + `sleep`/`wakeup` 的 console 讀取路徑不同,是這份 Rust 實作目前簡化過的地方。

#### virtio-blk 磁碟驅動

磁碟走 virtio-blk 協定(`kernel/src/drivers/virtio.rs`),對應到 `0x10001000`。virtio 用三個環形結構協調驅動與裝置的溝通,可以類比成餐廳的「點餐流程」:

- **descriptor table** 像是「一張張菜單項目卡」,每筆描述一段記憶體(位址、長度、讀/寫方向),可以用 `next` 串成一條鏈,代表一次請求裡的多個片段。
- **avail ring** 是驅動遞給裝置的「點餐單」:驅動把一條 descriptor 鏈的**頭**索引放進去,代表「這筆訂單準備好了,請裝置處理」。
- **used ring** 是裝置處理完後放回的「取餐號」:裝置處理完一筆請求,就把結果索引寫進這裡,驅動據此知道哪些請求已完成。

一次磁碟請求(`virtio_rw_buf`)的完整流程:準備 3 個 descriptor 串成一條鏈(標頭、資料緩衝、裝置回填的狀態碼)→ 把鏈頭索引寫進 avail ring 並把 `avail.idx` 加一 → 寫入 MMIO 的 `QUEUE_NOTIFY` 暫存器主動通知裝置 → **忙等輪詢** `used.idx` 直到它超過請求前記錄的值,代表裝置已經處理完。

```rust
let first_used = core::ptr::read_volatile(&(*used).idx);
(*avail).ring[(ai as usize) % QUEUE_SIZE] = 0;
(*avail).idx = ai.wrapping_add(1);
v.add(VIRTIO_MMIO_QUEUE_NOTIFY / 4).write_volatile(0);
while core::ptr::read_volatile(&(*used).idx) == first_used {
    core::hint::spin_loop();
}
```

這裡選擇「發出請求後忙等 used ring」而非「讓行程 sleep,由 virtio 中斷處理常式呼叫 wakeup」,是這份實作刻意簡化的設計:`virtio_intr()` 雖然存在、也會被 PLIC 分派到,但目前只做 ACK 與清空 used ring,並沒有真正拿來喚醒等待中的行程。整個操作是在持有 `VIRTIO_DEVICE` 的鎖之下進行,這代表任何時刻只會有一筆請求在飛行中——這是為了讓固定的 3-descriptor 鏈與簡單輪詢邏輯能夠成立。另外值得注意 `virtio_rw_buf` 的緩衝長度不限於 512 位元組:`buf.len() / 512` 決定一次請求要搬運幾個連續 sector,例如檔案系統的一個 1024-byte block 就是兩個連續 sector,寫入日誌區時甚至可以一次把整段連續區域打包成單一請求,省下多次 round-trip。

#### 中斷分派:PLIC

外部裝置(UART、virtio)的中斷不是直接送進 CPU,而是先經過 **PLIC**(Platform-Level Interrupt Controller,`kernel/src/arch/interrupt.rs`)。真正發生中斷時,`devintr()` 先判斷 `scause` 是否為 supervisor external interrupt,是的話呼叫 `plic_claim()` 問 PLIC「這次是哪個裝置的中斷」,依 IRQ 編號分派給對應驅動的中斷處理函式(`uart_intr`/`virtio_intr`),處理完再呼叫 `plic_complete(irq)` 通知 PLIC「這個中斷處理完了,可以再送下一個」——這一組 claim/complete 的握手正是 PLIC 分派外部裝置中斷的核心機制。

### Rust 版 vs 原始 C xv6 的差異小結

- **鎖的所有權模型**:C xv6 的鎖與資料是分開的兩個變數,靠程式設計師手動保證配對正確;Rust 版把資料包進鎖裡,透過 guard 的 RAII `Drop` 讓解鎖幾乎不可能忘記,編譯器也能藉由借用檢查阻止「忘記拿鎖就直接碰資料」。
- **`unsafe` 被侷限在少數地方**:MMIO 暫存器存取(`drivers/`)與鎖底層的原子操作、context 切換相關的裸指標操作(`sync/`、`arch/`)是少數必須用 `unsafe` 的地方,其餘邏輯(pipe 的環形緩衝讀寫、sleep/wakeup 的排程互動)都建立在這些安全抽象之上。
- **已知的簡化/差異**:console 讀取用輪詢＋`yield_now` 而非中斷驅動的 sleep/wakeup;virtio 磁碟 I/O 用忙等輪詢 used ring 而非中斷喚醒——這兩處都是與原始 C xv6(中斷驅動 I/O)行為不同、值得在效能討論中留意的地方。

---

## 結語:子系統如何協作

前面五章各自拆解了一個子系統,但作業系統的精妙之處,正在於它們**彼此交織**。回頭把整台機器當一個整體看,你會發現同樣幾個設計原則反覆出現。

**一次 `write("hello.txt")` 走遍了整台機器。** 使用者程式的 `write` 是一次 `ecall`——**陷阱機制(第 2 章)**把控制權從 U-mode 送進核心,存好暫存器、切好頁表;系統呼叫分派到 `sys_write`,它透過**檔案描述子(第 4 章)**找到背後的 inode,在 `begin_op`/`end_op` 的**交易(第 4 章 logging)**保護下,把資料先寫進 buffer cache、再由 WAL 保證斷電一致性;真正落盤時,呼叫**virtio 磁碟驅動(第 5 章)**把區塊送進硬體;這期間 buffer 的內容鎖可能讓行程**睡眠(第 5 章)**,於是**排程器(第 3 章)**切去跑別的行程;而這一切的記憶體存取,底層都由**虛擬記憶體(第 1 章)**的頁表默默轉譯。一個看似平凡的系統呼叫,串起了全部五章。

**三條反覆出現的主線:**

1. **隔離,靠虛擬記憶體與特權模式。** 每個行程活在自己的位址空間裡,碰不到別人的記憶體,也碰不到核心——這道牆由頁表(第 1 章)與 U/S-mode 的區隔(第 2 章)共同砌成,而 trampoline 是牆上唯一、受嚴格控管的門。

2. **正確性,靠鎖的紀律與 logging。** 多核心並行下,共享資料的每一次改動都發生在某把鎖的保護窗口內(第 5 章);而磁碟這種「改到一半斷電就毀了」的資源,則靠 write-ahead log 把「多步驟修改」變成「要嘛全發生、要嘛全沒發生」的原子交易(第 4 章)。「先設狀態再放鎖」「commit point 之前可捨棄、之後可重放」這些看似瑣碎的順序,正是正確性的來源。

3. **抽象分層,讓複雜度可控。** 檔案系統的七層、記憶體的頁框/頁表/heap 三層、鎖的 spinlock/sleeplock 兩層——每一層只依賴下一層的介面,不越級。這讓「換掉底層實作」變得可能(例如把 virtio 從忙等改成中斷驅動,上層完全無感)。

**而 Rust 帶來的,是把「紀律」變成「保證」。** 通篇你不斷看到同一個對照:C xv6 靠開發者的自律(記得配對 `acquire`/`release`、記得 `end_op`、記得別把虛擬位址當實體位址),Rust 則把這些自律搬進型別系統與 RAII——鎖 guard 離開作用域自動解鎖、`PageTable` 的 `Drop` 自動釋放、newtype 讓位址型別不會用錯、`enum` 逼你窮舉狀態。`unsafe` 沒有消失,但被收斂進 `arch/`、`mm/frame_allocator`、`sync/` 這幾個明確標示的角落,其餘核心程式碼得以站在安全抽象之上。

一個「小」作業系統,五臟俱全。讀到這裡,你已經走過了它的每一個房間。接下來最好的學習方式,就是打開 `kernel/src/`,挑一條你最好奇的路徑(不妨就從 `sys_write` 開始),對照著本文,一行一行走一遍——概念與程式碼之間的那層迷霧,會在你親手追蹤資料流的過程中散去。

### 延伸閱讀

- 原始 C xv6 與其配套講義《xv6: a simple, Unix-like teaching operating system》(MIT 6.1810/6.S081)——本文各章的概念源頭。
- 本專案 `docs/architecture.md`:更貼近程式碼的子系統參考(注意部分內容較舊,與實際程式碼衝突時以程式碼為準)。
- 本專案 `docs/perf-c-vs-rust.md`:C 版與 Rust 版的效能比較與核心 I/O 最佳化紀錄。
- RISC-V 特權架構手冊(*The RISC-V Instruction Set Manual, Volume II: Privileged Architecture*)——`satp`、`scause`、trap、PLIC 等硬體機制的權威定義。
