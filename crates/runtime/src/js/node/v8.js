// Minimal v8 module stub for utoo-runtime
function getHeapStatistics() {
  return {
    total_heap_size: 0,
    total_heap_size_executable: 0,
    total_physical_size: 0,
    total_available_size: 0,
    used_heap_size: 0,
    heap_size_limit: 0,
    malloced_memory: 0,
    peak_malloced_memory: 0,
    does_zap_garbage: 0,
    number_of_native_contexts: 0,
    number_of_detached_contexts: 0,
  };
}

function getHeapSpaceStatistics() {
  return [];
}

function getHeapCodeStatistics() {
  return { code_and_metadata_size: 0, bytecode_and_metadata_size: 0, external_script_source_size: 0 };
}

function setFlagsFromString() {}
function writeHeapSnapshot() { return ""; }

// node:v8 startupSnapshot API (Node-compatible subset). utoo-runtime drives
// these via globals: during a `snapshot` build it sets __utoo_building_snapshot
// and, after the entry runs, invokes the serialize callbacks then serializes the
// heap (capturing the registered deserialize main function + its data). During
// `run --snapshot` it invokes the deserialize callbacks and calls the
// deserialize main function. This lets frameworks (e.g. egg) that use Node's
// startup-snapshot lifecycle work unchanged on utoo-runtime.
const startupSnapshot = {
  isBuildingSnapshot() {
    return !!globalThis.__utoo_building_snapshot;
  },
  setDeserializeMainFunction(fn, data) {
    globalThis.__utoo_deserialize_main = fn;
    globalThis.__utoo_deserialize_data = data;
  },
  addSerializeCallback(fn, data) {
    if (!globalThis.__utoo_serialize_cbs) globalThis.__utoo_serialize_cbs = [];
    globalThis.__utoo_serialize_cbs.push([fn, data]);
  },
  addDeserializeCallback(fn, data) {
    if (!globalThis.__utoo_deserialize_cbs) globalThis.__utoo_deserialize_cbs = [];
    globalThis.__utoo_deserialize_cbs.push([fn, data]);
  },
};

const v8 = {
  getHeapStatistics,
  getHeapSpaceStatistics,
  getHeapCodeStatistics,
  setFlagsFromString,
  writeHeapSnapshot,
  startupSnapshot,
};
v8.default = v8;

export default v8;
export {
  getHeapStatistics,
  getHeapSpaceStatistics,
  getHeapCodeStatistics,
  setFlagsFromString,
  writeHeapSnapshot,
  startupSnapshot,
};
