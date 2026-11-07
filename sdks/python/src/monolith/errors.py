from __future__ import annotations

from dataclasses import dataclass, field
from typing import Any


@dataclass
class FieldProblem:
    field: str
    code: str
    message: str


@dataclass
class Problem:
    type: str
    title: str
    status: int
    detail: str | None = None
    request_id: str | None = None
    errors: list[FieldProblem] = field(default_factory=list)
    raw: dict[str, Any] = field(default_factory=dict)

    @property
    def requestId(self) -> str | None:
        return self.request_id

    @classmethod
    def from_dict(cls, data: dict[str, Any], status_code: int = 0) -> Problem:
        status = data.get("status", status_code)
        type_ = data.get("type", "about:blank")
        title = data.get("title", f"HTTP {status}" if status else "Error")
        detail = data.get("detail")
        request_id = data.get("requestId") or data.get("request_id")

        errors_raw = data.get("errors") or []
        errors: list[FieldProblem] = []
        if isinstance(errors_raw, list):
            for err in errors_raw:
                if isinstance(err, dict):
                    errors.append(
                        FieldProblem(
                            field=str(err.get("field", "")),
                            code=str(err.get("code", "")),
                            message=str(err.get("message", "")),
                        )
                    )

        return cls(
            type=type_,
            title=title,
            status=status,
            detail=detail,
            request_id=request_id,
            errors=errors,
            raw=data,
        )


class MonolithError(Exception):
    def __init__(self, problem: Problem | dict[str, Any]) -> None:
        if isinstance(problem, dict):
            problem = Problem.from_dict(problem)
        self.problem = problem
        self.status = problem.status
        self.type = problem.type
        self.detail = problem.detail
        self.request_id = problem.request_id
        self.errors = problem.errors

        message = problem.detail or problem.title or f"HTTP {problem.status}"
        super().__init__(message)

    @property
    def requestId(self) -> str | None:
        return self.request_id

    def get_field_error(self, field: str) -> str | None:
        for err in self.errors:
            if err.field == field:
                return err.message
        return None

    def __repr__(self) -> str:
        return (
            f"MonolithError(status={self.status}, type={self.type!r}, "
            f"detail={self.detail!r}, request_id={self.request_id!r})"
        )
