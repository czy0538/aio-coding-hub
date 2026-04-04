import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { keywordReviewKeys } from "./keys";
import {
  keywordReviewKeywordsList,
  keywordReviewKeywordAdd,
  keywordReviewKeywordSetEnabled,
  keywordReviewKeywordDelete,
  keywordReviewLogsList,
  keywordReviewDecide,
  keywordReviewPendingList,
} from "../services/keywordReview";

export function useKeywordReviewKeywordsQuery() {
  return useQuery({
    queryKey: keywordReviewKeys.keywords(),
    queryFn: keywordReviewKeywordsList,
  });
}

export function useKeywordReviewKeywordAddMutation() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (keyword: string) => keywordReviewKeywordAdd(keyword),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: keywordReviewKeys.keywords() }),
  });
}

export function useKeywordReviewKeywordSetEnabledMutation() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({ id, enabled }: { id: number; enabled: boolean }) =>
      keywordReviewKeywordSetEnabled(id, enabled),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: keywordReviewKeys.keywords() }),
  });
}

export function useKeywordReviewKeywordDeleteMutation() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (id: number) => keywordReviewKeywordDelete(id),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: keywordReviewKeys.keywords() }),
  });
}

export function useKeywordReviewLogsQuery(limit: number, offset: number) {
  return useQuery({
    queryKey: keywordReviewKeys.logs(limit, offset),
    queryFn: () => keywordReviewLogsList(limit, offset),
  });
}

export function useKeywordReviewDecideMutation() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({ traceId, decision }: { traceId: string; decision: "approve" | "reject" }) =>
      keywordReviewDecide(traceId, decision),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: keywordReviewKeys.pending() });
      queryClient.invalidateQueries({ queryKey: keywordReviewKeys.logs(50, 0) });
    },
  });
}

export function useKeywordReviewPendingQuery() {
  return useQuery({
    queryKey: keywordReviewKeys.pending(),
    queryFn: keywordReviewPendingList,
    refetchInterval: 5000,
  });
}
