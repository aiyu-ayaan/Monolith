/// RFC 9457 problem details error representation.

class FieldProblem {
  final String field;
  final String code;
  final String message;

  const FieldProblem({
    required this.field,
    required this.code,
    required this.message,
  });

  factory FieldProblem.fromJson(Map<String, dynamic> json) {
    return FieldProblem(
      field: json['field'] as String? ?? '',
      code: json['code'] as String? ?? '',
      message: json['message'] as String? ?? '',
    );
  }

  Map<String, dynamic> toJson() => {
        'field': field,
        'code': code,
        'message': message,
      };

  @override
  String toString() => 'FieldProblem($field: $code - $message)';
}

class Problem {
  final String type;
  final String title;
  final int status;
  final String? detail;
  final String? requestId;
  final List<FieldProblem> errors;
  final Map<String, dynamic> raw;

  const Problem({
    required this.type,
    required this.title,
    required this.status,
    this.detail,
    this.requestId,
    this.errors = const [],
    this.raw = const {},
  });

  factory Problem.fromJson(
    Map<String, dynamic> json, {
    int? defaultStatus,
    String? defaultRequestId,
  }) {
    final status = (json['status'] as num?)?.toInt() ?? defaultStatus ?? 0;
    final type = json['type'] as String? ?? 'about:blank';
    final title = json['title'] as String? ?? 'HTTP $status';
    final detail = json['detail'] as String?;
    final requestId = json['requestId'] as String? ??
        json['request_id'] as String? ??
        defaultRequestId;

    List<FieldProblem> errors = const [];
    if (json['errors'] is List) {
      errors = (json['errors'] as List)
          .whereType<Map<String, dynamic>>()
          .map(FieldProblem.fromJson)
          .toList();
    }

    return Problem(
      type: type,
      title: title,
      status: status,
      detail: detail,
      requestId: requestId,
      errors: errors,
      raw: json,
    );
  }

  Map<String, dynamic> toJson() => {
        'type': type,
        'title': title,
        'status': status,
        if (detail != null) 'detail': detail,
        if (requestId != null) 'requestId': requestId,
        if (errors.isNotEmpty) 'errors': errors.map((e) => e.toJson()).toList(),
        ...raw,
      };

  @override
  String toString() => 'Problem($status $title: $detail)';
}

class MonolithException implements Exception {
  final Problem problem;

  const MonolithException(this.problem);

  int get status => problem.status;
  String get type => problem.type;
  String get title => problem.title;
  String? get detail => problem.detail;
  String? get requestId => problem.requestId;
  List<FieldProblem> get errors => problem.errors;

  String? getFieldError(String field) {
    for (final err in errors) {
      if (err.field == field) return err.message;
    }
    return null;
  }

  @override
  String toString() {
    final msg = detail ?? title;
    final req = requestId != null ? ' (requestId: $requestId)' : '';
    return 'MonolithException: [$status] $msg$req';
  }
}

/// Convenience alias matching TypeScript SDK's MonolithError.
typedef MonolithError = MonolithException;
