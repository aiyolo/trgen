package parser

import "testing"

func TestParsesAndFlattensNestedStructs(t *testing.T) {
	source := `
            typedef struct {
                uint16_t code; // 状态码
                float voltage;
            } Sensor;

            typedef struct Device {
                uint32_t id;
                Sensor sensors[2];
                char name[8];
            } Device;
        `
	result, err := Parse(source, "Device")
	if err != nil {
		t.Fatal(err)
	}
	if result.RootName != "Device" {
		t.Fatalf("root = %q", result.RootName)
	}
	if result.TotalSize != 28 {
		t.Fatalf("total = %d", result.TotalSize)
	}
	if len(result.Rows) != 6 {
		t.Fatalf("rows = %d", len(result.Rows))
	}
	if result.Rows[1].ParameterName != "code" {
		t.Fatalf("row1 = %q", result.Rows[1].ParameterName)
	}
	if result.Rows[1].Comments != "状态码" {
		t.Fatalf("comments = %q", result.Rows[1].Comments)
	}
	if result.Rows[1].GroupKey != result.Rows[2].GroupKey {
		t.Fatal("group keys should match within nested struct")
	}
	if result.Rows[1].GroupKey == result.Rows[3].GroupKey {
		t.Fatal("group keys should differ across fields")
	}
	if result.Rows[0].GroupKey != result.Rows[5].GroupKey {
		t.Fatal("root fields share the root group key")
	}
	if result.Rows[3].IOBufferOffset != 12 {
		t.Fatalf("offset3 = %d", result.Rows[3].IOBufferOffset)
	}
	if result.Rows[5].DataSize != 64 {
		t.Fatalf("size5 = %d", result.Rows[5].DataSize)
	}
}

func TestSupportsNamedStructReferencesAndPointers(t *testing.T) {
	source := `
            struct Node {
                int value;
                struct Node *next;
            };
        `
	result, err := Parse(source, "")
	if err != nil {
		t.Fatal(err)
	}
	if result.TotalSize != 16 {
		t.Fatalf("total = %d", result.TotalSize)
	}
	if result.Rows[1].DataType != "POINTER" {
		t.Fatalf("type = %q", result.Rows[1].DataType)
	}
	if result.Rows[1].IOBufferOffset != 8 {
		t.Fatalf("offset = %d", result.Rows[1].IOBufferOffset)
	}
}

func TestUsesLastStructAsDefaultRoot(t *testing.T) {
	source := "typedef struct { int x; } A; typedef struct { A a; int y; } B;"
	result, err := Parse(source, "")
	if err != nil {
		t.Fatal(err)
	}
	if result.RootName != "B" {
		t.Fatalf("root = %q", result.RootName)
	}
	if result.Rows[0].ParameterName != "x" {
		t.Fatalf("row0 = %q", result.Rows[0].ParameterName)
	}
}

func TestParsesRealisticHeaderAndAllowsAnyRoot(t *testing.T) {
	source := `
            #ifndef DEVICE_PROTOCOL_H
            #define DEVICE_PROTOCOL_H
            #include <stdint.h>
            #define CHANNEL_COUNT (1U << 1)
            #define DEFAULT_SCALE 1.0f

            typedef unsigned char U8;
            typedef unsigned int U32;

            typedef struct __attribute__((packed)) _Sensor {
                U8 status;
                uint16_t code;
            } Sensor, *PSensor;

            typedef struct {
                U32 id;
                Sensor sensors[CHANNEL_COUNT];
            } Packet, *PPacket;

            int decode_packet(const Packet *packet);
            #endif
        `

	catalog, err := Discover(source)
	if err != nil {
		t.Fatal(err)
	}
	if len(catalog.AvailableStructs) != 2 || catalog.AvailableStructs[0] != "Sensor" || catalog.AvailableStructs[1] != "Packet" {
		t.Fatalf("catalog = %v", catalog.AvailableStructs)
	}
	if catalog.DefaultRoot != "Packet" {
		t.Fatalf("default = %q", catalog.DefaultRoot)
	}

	sensor, err := Parse(source, "Sensor")
	if err != nil {
		t.Fatal(err)
	}
	if sensor.RootName != "Sensor" || len(sensor.Rows) != 2 {
		t.Fatalf("sensor rows = %v", sensor.Rows)
	}
	if sensor.Rows[0].DataSize != 8 {
		t.Fatalf("size = %d", sensor.Rows[0].DataSize)
	}
	if sensor.Rows[1].IOBufferOffset != 1 {
		t.Fatalf("offset = %d", sensor.Rows[1].IOBufferOffset)
	}

	packet, err := Parse(source, "Packet")
	if err != nil {
		t.Fatal(err)
	}
	if len(packet.Rows) != 5 {
		t.Fatalf("packet rows = %d", len(packet.Rows))
	}
	if packet.Rows[1].ParameterName != "status" || packet.Rows[3].ParameterName != "status" {
		t.Fatalf("packet names = %v", packet.Rows)
	}
	if packet.TotalSize != 12 {
		t.Fatalf("packet total = %d", packet.TotalSize)
	}
}

func TestExpandsObjectFunctionAndMultilineMacros(t *testing.T) {
	source := `
            #define BASE_COUNT 2U
            #define CHANNEL_COUNT (BASE_COUNT * 2U)
            #define FIELD(type, name) type name;
            #define ARRAY_FIELD(type, name, count) \
                type name[count];
            #define COMMON_FIELDS \
                FIELD(uint32_t, id) \
                FIELD(uint8_t, mode)
            #define FLAG_BITS (1U + 2U)

            typedef struct {
                COMMON_FIELDS
                ARRAY_FIELD(uint16_t, samples, CHANNEL_COUNT)
                unsigned int flags : FLAG_BITS;
            } MacroRecord;
        `

	result, err := Parse(source, "MacroRecord")
	if err != nil {
		t.Fatal(err)
	}
	if len(result.Rows) != 4 {
		t.Fatalf("rows = %d", len(result.Rows))
	}
	if result.Rows[2].DataType != "UINT16[4]" {
		t.Fatalf("type = %q", result.Rows[2].DataType)
	}
	if result.Rows[2].DataSize != 64 {
		t.Fatalf("size = %d", result.Rows[2].DataSize)
	}
	if result.Rows[2].IOBufferOffset != 6 {
		t.Fatalf("offset = %d", result.Rows[2].IOBufferOffset)
	}
	if result.Rows[3].DataSize != 3 {
		t.Fatalf("bit size = %d", result.Rows[3].DataSize)
	}
	if result.Rows[3].IOBufferOffset != 16 {
		t.Fatalf("bit offset = %d", result.Rows[3].IOBufferOffset)
	}
	if result.TotalSize != 20 {
		t.Fatalf("total = %d", result.TotalSize)
	}
}

func TestHonorsConditionalMacros(t *testing.T) {
	source := `
            #define ENABLE_EXTRA 1
            #define MODE 2

            typedef struct {
                uint8_t prefix;
            #if ENABLE_EXTRA && MODE >= 2
                uint16_t enabled_value;
            #else
                uint64_t disabled_value;
            #endif
            #if 0
                uint32_t omitted_value;
            #endif
            #if defined(_WIN64) && !defined(NOT_DEFINED)
                uint32_t windows_value;
            #endif
            } ConditionalRecord;
        `

	result, err := Parse(source, "ConditionalRecord")
	if err != nil {
		t.Fatal(err)
	}
	if len(result.Rows) != 3 {
		t.Fatalf("rows = %v", result.Rows)
	}
	if result.Rows[1].ParameterName != "enabled_value" || result.Rows[1].IOBufferOffset != 2 {
		t.Fatalf("row1 = %+v", result.Rows[1])
	}
	if result.Rows[2].ParameterName != "windows_value" || result.Rows[2].IOBufferOffset != 4 {
		t.Fatalf("row2 = %+v", result.Rows[2])
	}
	if result.TotalSize != 8 {
		t.Fatalf("total = %d", result.TotalSize)
	}
}

func TestHonorsPragmaPackAndBitfields(t *testing.T) {
	source := `
            #pragma pack(push, 1)
            typedef struct {
                uint8_t first;
                uint32_t value;
                uint16_t last;
            } PackedRecord;

            typedef struct {
                uint8_t prefix;
                unsigned int low : 3;
                unsigned int high : 5;
                uint8_t suffix;
            } PackedBits;
            #pragma pack(pop)

            typedef struct {
                uint8_t first;
                uint32_t value;
                uint16_t last;
            } NaturalRecord;
        `

	packed, err := Parse(source, "PackedRecord")
	if err != nil {
		t.Fatal(err)
	}
	if packed.TotalSize != 7 || packed.Rows[1].IOBufferOffset != 1 || packed.Rows[2].IOBufferOffset != 5 {
		t.Fatalf("packed = %+v", packed)
	}

	bits, err := Parse(source, "PackedBits")
	if err != nil {
		t.Fatal(err)
	}
	if bits.TotalSize != 6 {
		t.Fatalf("bits total = %d", bits.TotalSize)
	}
	if bits.Rows[1].IOBufferOffset != 1 || bits.Rows[2].IOBufferOffset != 1 {
		t.Fatalf("bit offsets = %d/%d", bits.Rows[1].IOBufferOffset, bits.Rows[2].IOBufferOffset)
	}
	if bits.Rows[1].DataSize != 3 || bits.Rows[2].DataSize != 5 {
		t.Fatalf("bit sizes = %d/%d", bits.Rows[1].DataSize, bits.Rows[2].DataSize)
	}
	if bits.Rows[3].IOBufferOffset != 5 {
		t.Fatalf("suffix offset = %d", bits.Rows[3].IOBufferOffset)
	}

	natural, err := Parse(source, "NaturalRecord")
	if err != nil {
		t.Fatal(err)
	}
	if natural.TotalSize != 12 || natural.Rows[1].IOBufferOffset != 4 || natural.Rows[2].IOBufferOffset != 8 {
		t.Fatalf("natural = %+v", natural)
	}
}

func TestUsesUnionSizeWhenCalculatingOffsets(t *testing.T) {
	source := `
            typedef union {
                uint8_t byte_value;
                uint64_t wide_value;
            } Value;

            typedef struct {
                uint8_t tag;
                Value value;
                uint16_t tail;
            } WithUnion;
        `

	result, err := Parse(source, "WithUnion")
	if err != nil {
		t.Fatal(err)
	}
	if result.TotalSize != 24 {
		t.Fatalf("total = %d", result.TotalSize)
	}
	if result.Rows[1].IOBufferOffset != 8 || result.Rows[2].IOBufferOffset != 8 || result.Rows[3].IOBufferOffset != 16 {
		t.Fatalf("offsets = %+v", result.Rows)
	}
}

func TestCamelCaseJSONShape(t *testing.T) {
	result, err := Parse("typedef struct { uint32_t id; } Packet;", "")
	if err != nil {
		t.Fatal(err)
	}
	if result.Rows[0].ParameterName == "" || result.Rows[0].IOBufferOffset != 0 {
		t.Fatalf("row = %+v", result.Rows[0])
	}
}
