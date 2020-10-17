
GLSLC = glslc
OUTPUT = vert.spv frag.spv

.SUFFIXES: .vert .frag .spv

default: $(OUTPUT)

.vert.spv:
	$(GLSLC) -o $@ $<

.frag.spv:
	$(GLSLC) -o $@ $<
