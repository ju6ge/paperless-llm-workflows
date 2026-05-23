#!/usr/bin/env python3

from pydantic import BaseModel, TypeAdapter
from typing import Optional, List, Dict, Any, Tuple
from enum import Enum

class BenchmarkType(str, Enum):
    CustomFieldExtraction = "CustomFieldExtraction"
    CorrespondentSuggest = "CorrespondentSuggest"
    DecideValidCorrespondent = "DecideValidCorrespondent"
    DecideInvalidCorrespondent = "DecideInvalidCorrespondent"


BENCHMARK_TYPES = [
    BenchmarkType.CustomFieldExtraction,
    BenchmarkType.CorrespondentSuggest,
    BenchmarkType.DecideValidCorrespondent,
    BenchmarkType.DecideInvalidCorrespondent,
]


class TokenGenerationStats(BaseModel):
    prompt_tokens: int
    prompt_elapsed_ms: float
    injected_tokens: int
    injected_elapsed_ms: float
    sampled_tokens: int
    sampled_elapsed_ms: float
    forward_passes: int


class SingleResult(BaseModel):
    benchmark_type: BenchmarkType
    doc_id: int
    expected_result: Any
    benchmark_result: Any
    success: bool
    error: Optional[str]
    token_stats: Optional[TokenGenerationStats] = None


class BenchmarkResult(BaseModel):
    model: str
    results: List[SingleResult]


class ResultStats(BaseModel):
    success: int
    failure: int
    error: int
