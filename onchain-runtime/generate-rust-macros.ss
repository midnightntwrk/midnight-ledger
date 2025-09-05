; This file is part of midnight-ledger.
; Copyright (C) 2025 Midnight Foundation
; SPDX-License-Identifier: Apache-2.0
; Licensed under the Apache License, Version 2.0 (the "License");
; You may not use this file except in compliance with the License.
; You may obtain a copy of the License at
; http://www.apache.org/licenses/LICENSE-2.0
; Unless required by applicable law or agreed to in writing, software
; distributed under the License is distributed on an "AS IS" BASIS,
; WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
; See the License for the specific language governing permissions and
; limitations under the License.

(define-syntax nanopass-case
  (syntax-rules ()
    [(_ case-expr x arg ...) #'()]))

(define-syntax declare-ledger-type
  (syntax-rules ()
    [(_ type-name () scheme-type rust-type-expr)
     (define type-name rust-type-expr)]
    [(_ type-name (type-formal ...) scheme-type rust-type-expr)
     (define (type-name type-formal ...) rust-type-expr)]))

(define-syntax declare-ledger-adt
  (syntax-rules (Kernel initial-value)
    [(_ Kernel () description (initial-value #f) functions ...)
     (begin
       (declare-ledger-function Kernel () functions) ...)]
    [(_ adt-name ((meta-type tyarg) ...) description (initial-value op ...) functions ...)
     (begin
       (declare-ledger-function adt-name (tyarg ...) functions) ...)]))

(define-syntax declare-ledger-function
  (syntax-rules (Kernel function when js-only)
    [(_ adt-name (tyarg ...) (function js-only f-name ([arg-name arg-ty] ...) f-res doc-string (f-ops ...)))
     (void)]
    [(_ Kernel () (function class f-name ([arg-name arg-ty] ...) f-res doc-string (f-ops ...)))
     (output-function (format "kernel_~a" 'f-name) () ([arg-name arg-ty] ...) f-res (f-ops ...))]
    [(_ adt-name (tyarg ...) (function class f-name ([arg-name arg-ty] ...) f-res doc-string (f-ops ...)))
     (output-function (format "~a_~a" 'adt-name 'f-name) (tyarg ...) ([arg-name arg-ty] ...) f-res (f-ops ...))]
    [(_ adt-name (tyarg ...) (when condition functions ...))
     (begin
       (declare-ledger-function adt-name (tyarg ...) functions) ...)]))

(define-syntax path-arg
  (syntax-rules (quote stack)
    [(_ 'stack) "Key::Stack"]
    [(_ val) (format "Key::Value(~a.into())" (rt-arg val))]))

(define-syntax rt-arg
  (syntax-rules (void list length align state-value rt-entry-point-hash rt-aligned-concat rt-value->int rt-coin-commit rt-max-sizeof rt-leaf-hash quote f-cached f reverse cdr car + - * sub1 add1 expt null cell map array merkle-tree rt-null)
    [(_ #f) "false"]
    [(_ #t) "true"]
    [(_ (void)) "()"]
    [(_ (list entries ...))
      (format "vec![~{~a~^, ~}]" (list (path-arg entries) ...))]
    [(_ (align n bytes))
      (format "AlignedValue::from(~a as u~a)" (rt-arg n) (* 8 bytes))]
    [(_ (state-value 'null)) "StateValue::Null"]
    [(_ (state-value 'cell cont)) (format "StateValue::Cell(Sp::new(~a.try_into().unwrap()))" (rt-arg cont))]
    [(_ (state-value 'map ([key value] ...)))
      (format "StateValue::Map([~{~a~^, ~}].iter().cloned().collect())"
              (list (format "(AlignedValue::from(~a), ~a)" (rt-arg key) (rt-arg value)) ...))]
    [(_ (state-value 'array (entries ...)))
      (format "StateValue::Array(vec![~{~a~^, ~}].into())" (list (rt-arg entries) ...))]
    [(_ (state-value 'merkle-tree nat ([key value] ...)))
      (format "StateValue::BoundedMerkleTree(MerkleTree::blank(~a)~{.update(~a)~})"
              (rt-arg nat)
              (list (format "~a, ~a.into()" (rt-arg key) (rt-arg value)) ...))]
    [(_ (state-value 'ADT value value_type))
      (format "StateValue::from(~a)" (rt-arg value))]
    [(_ (rt-entry-point-hash ep))
      (format "AlignedValue::from(persistent_commit(&~a.0[..], HashOutput(*b\"midnight:entry-point\\0\\0\\0\\0\\0\\0\\0\\0\\0\\0\\0\\0\")))" (rt-arg ep))]
    [(_ (rt-aligned-concat args ...))
      (format "AlignedValue::concat([~{AlignedValue::from(~a)~^, ~}].iter())" (list (rt-arg args) ...))]
    [(_ (rt-value->int rt))
      (format "u32::try_from(~a).unwrap()" (rt-arg rt))]
    [(_ (rt-coin-commit coin recipient))
       (format "~a.commitment(&~a)" (rt-arg coin) (rt-arg recipient))]
    [(_ (rt-max-sizeof ty))
       (format "(<~a>::alignment().max_aligned_size() as u32)" (rt-arg ty))]
    [(_ (rt-leaf-hash item))
       (format "leaf_hash(&ValueReprAlignedValue(AlignedValue::from(~a)))" (rt-arg item))]
    [(_ 'stack)
      "Key::Stack"]
    [(_ f-cached) "$fcached"]
    [(_ f) "$f.clone()"]
    [(_ (length xs))
      (format "(~a.len() as u8)" (rt-arg xs))]
    [(_ (reverse xs))
      (format "~a.iter().cloned().rev().collect::<Vec<_>>()" (rt-arg xs))]
    [(_ (cdr xs))
      (format "~a.iter().cloned().skip(1).collect::<Vec<_>>()" (rt-arg xs))]
    [(_ (car xs))
      (format "~a[0].clone()" (rt-arg xs))]
    [(_ (+ args ...))
       (format "(~{~a~^ + ~})" (list (rt-arg args) ...))]
    [(_ (- arg))
       (format "(-~a)" (rt-arg arg))]
    [(_ (- args ...))
       (format "(~{~a~^ - ~})" (list (rt-arg args) ...))]
    [(_ (* args ...))
       (format "(~{~a~^ * ~})" (list (rt-arg args) ...))]
    [(_ (add1 arg))
       (format "(~a + 1)" (rt-arg arg))]
    [(_ (sub1 arg))
       (format "(~a - 1)" (rt-arg arg))]
    [(_ (expt a b))
       (format "(~a as u64).pow(~a)" (rt-arg a) (rt-arg b))]
    [(_ (rt-null ty))
       (format "AlignedValue::from(<~a>::default())" (rt-arg ty))]
    ;; NOTE: Doesn't actually suppress anything here, which may create invalid
    ;; instructions during tests.
    [(_ (suppress-null x))
     (rt-arg x)]
    [(_ (suppress-zero x))
     (rt-arg x)]
    [(_ arg) (if (or (number? arg)
                     (string? arg))
                 arg
                 (error 'rt-arg (format "Expected literal, got ~s" arg)))]))

(define (snake-case symbol)
  (define (snake-case* chars first)
    (cond
     [(null? chars) chars]
     [first
       (cons (char-downcase (car chars)) (snake-case* (cdr chars) #f))]
     [(char-upper-case? (car chars))
       (list* #\_ (char-downcase (car chars)) (snake-case* (cdr chars) #f))]
     [else (cons (car chars) (snake-case* (cdr chars) #f))]))
  (string->symbol (list->string (snake-case* (string->list (symbol->string symbol)) #t))))

(define-syntax output-function
  (syntax-rules ()
    [(_ name-expr (tyarg ...) ([arg-name arg-ty] ...) f-res ((op [op-arg-name op-arg-val] ...) ...))
     (begin
       (printf "#[macro_export]\n")
       (printf "macro_rules! ~a {\n" name-expr)
       (printf "  ($f:expr, $fcached:expr~{, ~a~}~{, $~a:expr~}) => {\n"
               (list (if (eq? 'nat 'tyarg)
                         (format "$~a:literal" 'tyarg)
                         (format "$~a:ty" 'tyarg)) ...)
               '(arg-name ...))
       (printf "    [\n")
       (begin
         (let* ([arg-name (format "$~a.clone()" 'arg-name)] ...
                [tyarg (format "$~a" 'tyarg)] ...
                [arg-str (if (zero? (length '(op-arg-name ...)))
                             ""
                             (format " { ~{~a~^, ~} }"
                                     (map
                                       (lambda (name val)
                                         (format "~a: ~a.try_into().unwrap()" (snake-case name) val))
                                       '(op-arg-name ...)
                                       (list (rt-arg op-arg-val) ...))))])
           (printf "      Op::~a~a,\n"
                   (string->symbol (string-titlecase (symbol->string 'op)))
                   arg-str))) ...
       (printf "    ]\n")
       (printf "  };\n")
       (printf "}\n")
       (printf "pub use ~a;\n" name-expr))]))

(include "../.build/midnight-ledger.ss")
