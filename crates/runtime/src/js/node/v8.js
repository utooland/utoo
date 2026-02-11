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

const v8 = {
  getHeapStatistics,
  getHeapSpaceStatistics,
  getHeapCodeStatistics,
  setFlagsFromString,
  writeHeapSnapshot,
};
v8.default = v8;

export default v8;
export {
  getHeapStatistics,
  getHeapSpaceStatistics,
  getHeapCodeStatistics,
  setFlagsFromString,
  writeHeapSnapshot,
};
