@0x85150b117366d14b;

enum Operation {
  add @0;
  sub @1;
  mul @2;
  div @3;
}

struct CalcRequest {
  op @0 :Operation;
  a @1 :Float64;
  b @2 :Float64;
  iters @3 :UInt32;
}

struct CalcResponse {
  result @0 :Float64;
  computeTimeUs @1 :UInt64;  # Optional: server-side timing
}
