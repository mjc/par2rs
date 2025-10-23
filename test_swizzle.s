	.file	"test_swizzle.339a020225b21f88-cgu.0"
	.section	.text._ZN12test_swizzle16test_swizzle_dyn17hf722e944ca34f639E,"ax",@progbits
	.globl	_ZN12test_swizzle16test_swizzle_dyn17hf722e944ca34f639E
	.p2align	4
	.type	_ZN12test_swizzle16test_swizzle_dyn17hf722e944ca34f639E,@function
_ZN12test_swizzle16test_swizzle_dyn17hf722e944ca34f639E:
	.cfi_startproc
	vmovdqa	(%rdx), %xmm1
	vmovdqa	(%rsi), %xmm0
	movq	%rdi, %rax
	vmovd	%xmm1, %ecx
	vmovdqa	%xmm0, -24(%rsp)
	vpxor	%xmm0, %xmm0, %xmm0
	cmpb	$16, %cl
	jb	.LBB0_32
	vpextrb	$1, %xmm1, %ecx
	cmpb	$15, %cl
	jbe	.LBB0_2
.LBB0_3:
	vpextrb	$2, %xmm1, %ecx
	cmpb	$15, %cl
	jbe	.LBB0_4
.LBB0_5:
	vpextrb	$3, %xmm1, %ecx
	cmpb	$15, %cl
	jbe	.LBB0_6
.LBB0_7:
	vpextrb	$4, %xmm1, %ecx
	cmpb	$15, %cl
	jbe	.LBB0_8
.LBB0_9:
	vpextrb	$5, %xmm1, %ecx
	cmpb	$15, %cl
	jbe	.LBB0_10
.LBB0_11:
	vpextrb	$6, %xmm1, %ecx
	cmpb	$15, %cl
	jbe	.LBB0_12
.LBB0_13:
	vpextrb	$7, %xmm1, %ecx
	cmpb	$15, %cl
	jbe	.LBB0_14
.LBB0_15:
	vpextrb	$8, %xmm1, %ecx
	cmpb	$15, %cl
	jbe	.LBB0_16
.LBB0_17:
	vpextrb	$9, %xmm1, %ecx
	cmpb	$15, %cl
	jbe	.LBB0_18
.LBB0_19:
	vpextrb	$10, %xmm1, %ecx
	cmpb	$15, %cl
	jbe	.LBB0_20
.LBB0_21:
	vpextrb	$11, %xmm1, %ecx
	cmpb	$15, %cl
	jbe	.LBB0_22
.LBB0_23:
	vpextrb	$12, %xmm1, %ecx
	cmpb	$15, %cl
	jbe	.LBB0_24
.LBB0_25:
	vpextrb	$13, %xmm1, %ecx
	cmpb	$15, %cl
	jbe	.LBB0_26
.LBB0_27:
	vpextrb	$14, %xmm1, %ecx
	cmpb	$15, %cl
	jbe	.LBB0_28
.LBB0_29:
	vpextrb	$15, %xmm1, %ecx
	cmpb	$15, %cl
	jbe	.LBB0_30
.LBB0_31:
	vmovdqa	%xmm0, (%rax)
	retq
.LBB0_32:
	movzbl	%cl, %ecx
	movzbl	-24(%rsp,%rcx), %ecx
	vmovd	%ecx, %xmm0
	vpextrb	$1, %xmm1, %ecx
	cmpb	$15, %cl
	ja	.LBB0_3
.LBB0_2:
	movzbl	%cl, %ecx
	vpinsrb	$1, -24(%rsp,%rcx), %xmm0, %xmm0
	vpextrb	$2, %xmm1, %ecx
	cmpb	$15, %cl
	ja	.LBB0_5
.LBB0_4:
	movzbl	%cl, %ecx
	vpinsrb	$2, -24(%rsp,%rcx), %xmm0, %xmm0
	vpextrb	$3, %xmm1, %ecx
	cmpb	$15, %cl
	ja	.LBB0_7
.LBB0_6:
	movzbl	%cl, %ecx
	vpinsrb	$3, -24(%rsp,%rcx), %xmm0, %xmm0
	vpextrb	$4, %xmm1, %ecx
	cmpb	$15, %cl
	ja	.LBB0_9
.LBB0_8:
	movzbl	%cl, %ecx
	vpinsrb	$4, -24(%rsp,%rcx), %xmm0, %xmm0
	vpextrb	$5, %xmm1, %ecx
	cmpb	$15, %cl
	ja	.LBB0_11
.LBB0_10:
	movzbl	%cl, %ecx
	vpinsrb	$5, -24(%rsp,%rcx), %xmm0, %xmm0
	vpextrb	$6, %xmm1, %ecx
	cmpb	$15, %cl
	ja	.LBB0_13
.LBB0_12:
	movzbl	%cl, %ecx
	vpinsrb	$6, -24(%rsp,%rcx), %xmm0, %xmm0
	vpextrb	$7, %xmm1, %ecx
	cmpb	$15, %cl
	ja	.LBB0_15
.LBB0_14:
	movzbl	%cl, %ecx
	vpinsrb	$7, -24(%rsp,%rcx), %xmm0, %xmm0
	vpextrb	$8, %xmm1, %ecx
	cmpb	$15, %cl
	ja	.LBB0_17
.LBB0_16:
	movzbl	%cl, %ecx
	vpinsrb	$8, -24(%rsp,%rcx), %xmm0, %xmm0
	vpextrb	$9, %xmm1, %ecx
	cmpb	$15, %cl
	ja	.LBB0_19
.LBB0_18:
	movzbl	%cl, %ecx
	vpinsrb	$9, -24(%rsp,%rcx), %xmm0, %xmm0
	vpextrb	$10, %xmm1, %ecx
	cmpb	$15, %cl
	ja	.LBB0_21
.LBB0_20:
	movzbl	%cl, %ecx
	vpinsrb	$10, -24(%rsp,%rcx), %xmm0, %xmm0
	vpextrb	$11, %xmm1, %ecx
	cmpb	$15, %cl
	ja	.LBB0_23
.LBB0_22:
	movzbl	%cl, %ecx
	vpinsrb	$11, -24(%rsp,%rcx), %xmm0, %xmm0
	vpextrb	$12, %xmm1, %ecx
	cmpb	$15, %cl
	ja	.LBB0_25
.LBB0_24:
	movzbl	%cl, %ecx
	vpinsrb	$12, -24(%rsp,%rcx), %xmm0, %xmm0
	vpextrb	$13, %xmm1, %ecx
	cmpb	$15, %cl
	ja	.LBB0_27
.LBB0_26:
	movzbl	%cl, %ecx
	vpinsrb	$13, -24(%rsp,%rcx), %xmm0, %xmm0
	vpextrb	$14, %xmm1, %ecx
	cmpb	$15, %cl
	ja	.LBB0_29
.LBB0_28:
	movzbl	%cl, %ecx
	vpinsrb	$14, -24(%rsp,%rcx), %xmm0, %xmm0
	vpextrb	$15, %xmm1, %ecx
	cmpb	$15, %cl
	ja	.LBB0_31
.LBB0_30:
	movzbl	%cl, %ecx
	vpinsrb	$15, -24(%rsp,%rcx), %xmm0, %xmm0
	vmovdqa	%xmm0, (%rax)
	retq
.Lfunc_end0:
	.size	_ZN12test_swizzle16test_swizzle_dyn17hf722e944ca34f639E, .Lfunc_end0-_ZN12test_swizzle16test_swizzle_dyn17hf722e944ca34f639E
	.cfi_endproc

	.section	.text._ZN12test_swizzle17test_shuffle_epi817h250693170fbd625eE,"ax",@progbits
	.globl	_ZN12test_swizzle17test_shuffle_epi817h250693170fbd625eE
	.p2align	4
	.type	_ZN12test_swizzle17test_shuffle_epi817h250693170fbd625eE,@function
_ZN12test_swizzle17test_shuffle_epi817h250693170fbd625eE:
	.cfi_startproc
	vmovdqa	(%rsi), %xmm0
	movq	%rdi, %rax
	vpshufb	(%rdx), %xmm0, %xmm0
	vmovdqa	%xmm0, (%rdi)
	retq
.Lfunc_end1:
	.size	_ZN12test_swizzle17test_shuffle_epi817h250693170fbd625eE, .Lfunc_end1-_ZN12test_swizzle17test_shuffle_epi817h250693170fbd625eE
	.cfi_endproc

	.ident	"rustc version 1.92.0-nightly (f04e3dfc8 2025-10-19)"
	.section	".note.GNU-stack","",@progbits
