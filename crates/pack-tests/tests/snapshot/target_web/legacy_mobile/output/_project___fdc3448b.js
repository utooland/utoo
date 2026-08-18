(globalThis["TURBOPACK"] || (globalThis["TURBOPACK"] = [])).push([typeof document === "object" ? document.currentScript : undefined,
"[project]/node_modules/@swc/helpers/_/_async_to_generator.js [client] (ecmascript)", (function(__turbopack_context__){
"use strict";

function _(fn) {
    return function() {
        return fn.apply(this, arguments);
    };
}
__turbopack_context__.s([
    "_",
    0,
    _
]);
}),
"[project]/node_modules/@swc/helpers/_/_class_call_check.js [client] (ecmascript)", (function(__turbopack_context__){
"use strict";

function _() {
    return 'purposefully empty stub for @swc/helpers/_/_class_call_check.js';
}
__turbopack_context__.s([
    "_",
    0,
    _
]);
}),
"[project]/node_modules/@swc/helpers/_/_create_class.js [client] (ecmascript)", (function(__turbopack_context__){
"use strict";

function _(Ctor, protoProps) {
    var _iteratorNormalCompletion = true, _didIteratorError = false, _iteratorError = undefined;
    try {
        for(var _iterator = (protoProps || [])[Symbol.iterator](), _step; !(_iteratorNormalCompletion = (_step = _iterator.next()).done); _iteratorNormalCompletion = true){
            var prop = _step.value;
            Object.defineProperty(Ctor.prototype, prop.key, {
                value: prop.value,
                configurable: true,
                writable: true
            });
        }
    } catch (err) {
        _didIteratorError = true;
        _iteratorError = err;
    } finally{
        try {
            if (!_iteratorNormalCompletion && _iterator.return != null) {
                _iterator.return();
            }
        } finally{
            if (_didIteratorError) {
                throw _iteratorError;
            }
        }
    }
    return Ctor;
}
__turbopack_context__.s([
    "_",
    0,
    _
]);
}),
"[project]/node_modules/@swc/helpers/_/_object_spread.js [client] (ecmascript)", (function(__turbopack_context__){
"use strict";

function _(target, source) {
    return Object.assign(target, source);
}
__turbopack_context__.s([
    "_",
    0,
    _
]);
}),
"[project]/node_modules/@swc/helpers/_/_object_spread_props.js [client] (ecmascript)", (function(__turbopack_context__){
"use strict";

function _(target, props) {
    return Object.assign(target, props);
}
__turbopack_context__.s([
    "_",
    0,
    _
]);
}),
"[project]/node_modules/@swc/helpers/_/_to_consumable_array.js [client] (ecmascript)", (function(__turbopack_context__){
"use strict";

function _(value) {
    if (Array.isArray(value)) {
        return value.slice();
    }
    return Array.from(value);
}
__turbopack_context__.s([
    "_",
    0,
    _
]);
}),
"[project]/node_modules/@swc/helpers/_/_ts_generator.js [client] (ecmascript)", (function(__turbopack_context__){
"use strict";

function _(thisArg, body) {
    return body.call(thisArg, {
        label: 0,
        sent: function sent() {
            return undefined;
        },
        tries: [],
        ops: []
    });
}
__turbopack_context__.s([
    "_",
    0,
    _
]);
}),
"[project]/target_web/legacy_mobile/input/async_module.js [client] (ecmascript)", (function(__turbopack_context__){
"use strict";

return __turbopack_context__.a(function(__turbopack_handle_async_dependencies__, __turbopack_async_result__) {
    var __gen = function*() {
        try {
            var data = yield fetch("/api/config").then(function(r) {
                return r.json();
            });
            var config = data;
            var version = "1.0.0";
            __turbopack_context__.s([
                "config",
                0,
                config,
                "version",
                0,
                version
            ]);
            __turbopack_async_result__();
        } catch (e) {
            __turbopack_async_result__(e);
        }
    }();
    (function __step(k, a) {
        try {
            var r = __gen[k](a);
        } catch (e) {
            __turbopack_async_result__(e);
            return;
        }
        if (!r.done) Promise.resolve(r.value).then(function(v) {
            __step('next', v);
        }, function(e) {
            __step('throw', e);
        });
    })('next');
}, true);
}),
"[project]/target_web/legacy_mobile/input/index.js [client] (ecmascript)", (function(__turbopack_context__){
"use strict";

return __turbopack_context__.a(function(__turbopack_handle_async_dependencies__, __turbopack_async_result__) {
    var __gen = function*() {
        try {
            var __TURBOPACK__imported__module__$5b$project$5d2f$node_modules$2f40$swc$2f$helpers$2f$_$2f$_async_to_generator$2e$js__$5b$client$5d$__$28$ecmascript$29$__ = __turbopack_context__.i("[project]/node_modules/@swc/helpers/_/_async_to_generator.js [client] (ecmascript)");
            var __TURBOPACK__imported__module__$5b$project$5d2f$node_modules$2f40$swc$2f$helpers$2f$_$2f$_class_call_check$2e$js__$5b$client$5d$__$28$ecmascript$29$__ = __turbopack_context__.i("[project]/node_modules/@swc/helpers/_/_class_call_check.js [client] (ecmascript)");
            var __TURBOPACK__imported__module__$5b$project$5d2f$node_modules$2f40$swc$2f$helpers$2f$_$2f$_create_class$2e$js__$5b$client$5d$__$28$ecmascript$29$__ = __turbopack_context__.i("[project]/node_modules/@swc/helpers/_/_create_class.js [client] (ecmascript)");
            var __TURBOPACK__imported__module__$5b$project$5d2f$node_modules$2f40$swc$2f$helpers$2f$_$2f$_object_spread$2e$js__$5b$client$5d$__$28$ecmascript$29$__ = __turbopack_context__.i("[project]/node_modules/@swc/helpers/_/_object_spread.js [client] (ecmascript)");
            var __TURBOPACK__imported__module__$5b$project$5d2f$node_modules$2f40$swc$2f$helpers$2f$_$2f$_object_spread_props$2e$js__$5b$client$5d$__$28$ecmascript$29$__ = __turbopack_context__.i("[project]/node_modules/@swc/helpers/_/_object_spread_props.js [client] (ecmascript)");
            var __TURBOPACK__imported__module__$5b$project$5d2f$node_modules$2f40$swc$2f$helpers$2f$_$2f$_to_consumable_array$2e$js__$5b$client$5d$__$28$ecmascript$29$__ = __turbopack_context__.i("[project]/node_modules/@swc/helpers/_/_to_consumable_array.js [client] (ecmascript)");
            var __TURBOPACK__imported__module__$5b$project$5d2f$node_modules$2f40$swc$2f$helpers$2f$_$2f$_ts_generator$2e$js__$5b$client$5d$__$28$ecmascript$29$__ = __turbopack_context__.i("[project]/node_modules/@swc/helpers/_/_ts_generator.js [client] (ecmascript)");
            var __TURBOPACK__imported__module__$5b$project$5d2f$target_web$2f$legacy_mobile$2f$input$2f$async_module$2e$js__$5b$client$5d$__$28$ecmascript$29$__ = __turbopack_context__.i("[project]/target_web/legacy_mobile/input/async_module.js [client] (ecmascript)");
            var __turbopack_async_dependencies__ = __turbopack_handle_async_dependencies__([
                __TURBOPACK__imported__module__$5b$project$5d2f$target_web$2f$legacy_mobile$2f$input$2f$async_module$2e$js__$5b$client$5d$__$28$ecmascript$29$__
            ]);
            [__TURBOPACK__imported__module__$5b$project$5d2f$target_web$2f$legacy_mobile$2f$input$2f$async_module$2e$js__$5b$client$5d$__$28$ecmascript$29$__] = __turbopack_async_dependencies__.then ? (yield __turbopack_async_dependencies__)() : __turbopack_async_dependencies__;
            ;
            ;
            ;
            ;
            ;
            ;
            ;
            ;
            console.log("config loaded:", __TURBOPACK__imported__module__$5b$project$5d2f$target_web$2f$legacy_mobile$2f$input$2f$async_module$2e$js__$5b$client$5d$__$28$ecmascript$29$__["config"], "version:", __TURBOPACK__imported__module__$5b$project$5d2f$target_web$2f$legacy_mobile$2f$input$2f$async_module$2e$js__$5b$client$5d$__$28$ecmascript$29$__["version"]);
            var LegacyBox = /*#__PURE__*/ function() {
                "use strict";
                function LegacyBox(value) {
                    (0, __TURBOPACK__imported__module__$5b$project$5d2f$node_modules$2f40$swc$2f$helpers$2f$_$2f$_class_call_check$2e$js__$5b$client$5d$__$28$ecmascript$29$__["_"])(this, LegacyBox);
                    this.value = value;
                }
                (0, __TURBOPACK__imported__module__$5b$project$5d2f$node_modules$2f40$swc$2f$helpers$2f$_$2f$_create_class$2e$js__$5b$client$5d$__$28$ecmascript$29$__["_"])(LegacyBox, [
                    {
                        key: "toJSON",
                        value: function toJSON() {
                            return {
                                value: this.value
                            };
                        }
                    }
                ]);
                return LegacyBox;
            }();
            function loadProfile(rawUser) {
                return (0, __TURBOPACK__imported__module__$5b$project$5d2f$node_modules$2f40$swc$2f$helpers$2f$_$2f$_async_to_generator$2e$js__$5b$client$5d$__$28$ecmascript$29$__["_"])(function() {
                    var _ref, _ref1, _rawUser_profile, lazy, name, tags, profile, box;
                    return (0, __TURBOPACK__imported__module__$5b$project$5d2f$node_modules$2f40$swc$2f$helpers$2f$_$2f$_ts_generator$2e$js__$5b$client$5d$__$28$ecmascript$29$__["_"])(this, function(_state) {
                        switch(_state.label){
                            case 0:
                                return [
                                    4,
                                    __turbopack_context__.A("[project]/target_web/legacy_mobile/input/lazy.js [client] (ecmascript, async loader)")
                                ];
                            case 1:
                                lazy = _state.sent();
                                name = (_ref = rawUser === null || rawUser === void 0 ? void 0 : (_rawUser_profile = rawUser.profile) === null || _rawUser_profile === void 0 ? void 0 : _rawUser_profile.name) !== null && _ref !== void 0 ? _ref : "guest";
                                tags = (0, __TURBOPACK__imported__module__$5b$project$5d2f$node_modules$2f40$swc$2f$helpers$2f$_$2f$_to_consumable_array$2e$js__$5b$client$5d$__$28$ecmascript$29$__["_"])((_ref1 = rawUser === null || rawUser === void 0 ? void 0 : rawUser.tags) !== null && _ref1 !== void 0 ? _ref1 : []).concat((0, __TURBOPACK__imported__module__$5b$project$5d2f$node_modules$2f40$swc$2f$helpers$2f$_$2f$_to_consumable_array$2e$js__$5b$client$5d$__$28$ecmascript$29$__["_"])(lazy.extraTags));
                                profile = (0, __TURBOPACK__imported__module__$5b$project$5d2f$node_modules$2f40$swc$2f$helpers$2f$_$2f$_object_spread_props$2e$js__$5b$client$5d$__$28$ecmascript$29$__["_"])((0, __TURBOPACK__imported__module__$5b$project$5d2f$node_modules$2f40$swc$2f$helpers$2f$_$2f$_object_spread$2e$js__$5b$client$5d$__$28$ecmascript$29$__["_"])({}, lazy.defaults), {
                                    name: name,
                                    tags: tags
                                });
                                box = new LegacyBox(profile);
                                return [
                                    2,
                                    box.toJSON()
                                ];
                        }
                    });
                })();
            }
            loadProfile({
                profile: {
                    name: "utoo"
                },
                tags: [
                    "pack"
                ]
            }).then(function(profile) {
                console.log(profile.value.name, profile.value.tags.join(","), profile.value.tags.includes("legacy"));
            });
            __turbopack_context__.s([
                "loadProfile",
                0,
                loadProfile
            ]);
            __turbopack_async_result__();
        } catch (e) {
            __turbopack_async_result__(e);
        }
    }();
    (function __step(k, a) {
        try {
            var r = __gen[k](a);
        } catch (e) {
            __turbopack_async_result__(e);
            return;
        }
        if (!r.done) Promise.resolve(r.value).then(function(v) {
            __step('next', v);
        }, function(e) {
            __step('throw', e);
        });
    })('next');
}, false);
}),
"[project]/target_web/legacy_mobile/input/lazy.js [client] (ecmascript, async loader)", (function(__turbopack_context__){

__turbopack_context__.v(function(parentImport) {
    return Promise.all([
  "input_lazy_456d4ef4.js"
].map(function(chunk) { return __turbopack_context__.l(chunk); })).then(function() {
        return parentImport("[project]/target_web/legacy_mobile/input/lazy.js [client] (ecmascript)");
    });
});
}),
]);

//# sourceMappingURL=_project___fdc3448b.js.map