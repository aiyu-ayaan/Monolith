export interface FieldProblem {
  field: string;
  code: string;
  message: string;
}

export interface Problem {
  type: string;
  title: string;
  status: number;
  detail?: string;
  requestId?: string;
  errors?: FieldProblem[];
  [key: string]: unknown;
}

export class MonolithError extends Error {
  readonly status: number;
  readonly type: string;
  readonly detail?: string;
  readonly requestId?: string;
  readonly errors: FieldProblem[];
  readonly problem: Problem;

  constructor(problem: Problem) {
    super(problem.detail ?? problem.title ?? `HTTP ${problem.status}`);
    this.name = "MonolithError";
    this.status = problem.status;
    this.type = problem.type;
    this.detail = problem.detail;
    this.requestId = problem.requestId;
    this.errors = problem.errors ?? [];
    this.problem = problem;
  }

  getFieldError(field: string): string | undefined {
    return this.errors.find((e) => e.field === field)?.message;
  }
}

