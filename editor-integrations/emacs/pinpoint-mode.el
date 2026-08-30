;;; pinpoint-mode.el --- Major mode for Pinpoint presentations -*- lexical-binding: t; -*-

;; This file is part of Pinpoint and is distributed under LGPL-2.1-or-later.

(define-derived-mode pinpoint-mode text-mode "Pinpoint"
  "A concise major mode for Pinpoint .pin files."
  (setq-local comment-start "# ")
  (setq-local font-lock-defaults
              '((
                 ("^#.*$" . font-lock-comment-face)
                 ("^-+.*$" . font-lock-keyword-face)
                 ("\\[[^]]+\\]" . font-lock-type-face)
                 ("</?\\(markup\\|span\\|b\\|big\\|i\\|s\\|sub\\|sup\\|small\\|tt\\|u\\)\\(?:[ >]\\)"
                  1 font-lock-function-name-face))))

  (setq-local completion-at-point-functions '(pinpoint-completion-at-point)))

(defun pinpoint-completion-at-point ()
  "Offer non-intrusive format snippets at point."
  (let ((start (point))
        (items '("--" "#" "#@alt:" "[duration=10]" "[text-align=center]"
                 "[transition=slide-in-left]" "<b></b>" "<span></span>")))
    (list start start items :exclusive 'no)))

;;;###autoload
(add-to-list 'auto-mode-alist '("\\.pin\\'" . pinpoint-mode))

(provide 'pinpoint-mode)
;;; pinpoint-mode.el ends here
