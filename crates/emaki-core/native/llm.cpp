// The dictation's clean-up model, run by libllama (vendor/llama.cpp) on
// the ggml transcribe.cpp brings. Three calls for `clean.rs`: load a
// model, complete a prompt already written out in the model's own chat
// form, and let the model go. The structs llama.h passes by value stay
// on this side, so the Rust side needs no copy of their layout.

#include "llama.h"

#include <cstring>
#include <mutex>
#include <string>
#include <vector>

struct emaki_llm {
    llama_model * model;
    llama_context * ctx;
    const llama_vocab * vocab;
};

static void quiet(enum ggml_log_level level, const char * text, void *) {
    if (level == GGML_LOG_LEVEL_ERROR && text) {
        fputs(text, stderr);
    }
}

extern "C" {

// A model from its GGUF file, every layer on the GPU, with room for
// `ctx_tokens` of prompt and answer together. Null when it cannot be
// read or no context can be made for it.
emaki_llm * emaki_llm_load(const char * path, int ctx_tokens) {
    static std::once_flag once;
    std::call_once(once, [] {
        llama_log_set(quiet, nullptr);
        llama_backend_init();
    });
    llama_model_params mp = llama_model_default_params();
    mp.n_gpu_layers = 999;
    llama_model * model = llama_model_load_from_file(path, mp);
    if (!model) {
        return nullptr;
    }
    llama_context_params cp = llama_context_default_params();
    cp.n_ctx = (uint32_t) ctx_tokens;
    cp.n_batch = (uint32_t) ctx_tokens;
    cp.no_perf = true;
    llama_context * ctx = llama_init_from_model(model, cp);
    if (!ctx) {
        llama_model_free(model);
        return nullptr;
    }
    return new emaki_llm{model, ctx, llama_model_get_vocab(model)};
}

// What the model writes after `prompt`, greedily, up to `max_new` tokens
// or its end of turn, into `out` (no more than `out_cap` bytes, with no
// terminator). The number of bytes written; -1 when the model failed,
// -2 when the prompt and the answer would not fit.
int emaki_llm_complete(emaki_llm * h, const char * prompt, int max_new, char * out, int out_cap) {
    llama_memory_clear(llama_get_memory(h->ctx), true);
    const int len = (int) strlen(prompt);
    const int n = -llama_tokenize(h->vocab, prompt, len, nullptr, 0, true, true);
    if (n <= 0) {
        return -1;
    }
    std::vector<llama_token> tokens(n);
    if (llama_tokenize(h->vocab, prompt, len, tokens.data(), n, true, true) < 0) {
        return -1;
    }
    if (n + max_new > (int) llama_n_ctx(h->ctx)) {
        return -2;
    }
    if (llama_decode(h->ctx, llama_batch_get_one(tokens.data(), n)) != 0) {
        return -1;
    }
    llama_sampler * sampler = llama_sampler_chain_init(llama_sampler_chain_default_params());
    llama_sampler_chain_add(sampler, llama_sampler_init_greedy());
    std::string said;
    for (int i = 0; i < max_new; i++) {
        llama_token token = llama_sampler_sample(sampler, h->ctx, -1);
        if (llama_vocab_is_eog(h->vocab, token)) {
            break;
        }
        char piece[256];
        const int k = llama_token_to_piece(h->vocab, token, piece, sizeof piece, 0, false);
        if (k > 0) {
            said.append(piece, k);
        }
        if (llama_decode(h->ctx, llama_batch_get_one(&token, 1)) != 0) {
            break;
        }
    }
    llama_sampler_free(sampler);
    const int wrote = (int) said.size() < out_cap ? (int) said.size() : out_cap;
    memcpy(out, said.data(), wrote);
    return wrote;
}

void emaki_llm_free(emaki_llm * h) {
    if (!h) {
        return;
    }
    llama_free(h->ctx);
    llama_model_free(h->model);
    delete h;
}

}
