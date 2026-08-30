if exists('b:current_syntax')
  finish
endif

syntax match pinpointNote /^#.*$/
syntax match pinpointSeparator /^-\+.*/
syntax match pinpointSetting /\[[^]]\+\]/
syntax match pinpointTag /<\/?\%(markup\|span\|b\|big\|i\|s\|sub\|sup\|small\|tt\|u\)\%([ >]\)/
syntax match pinpointEscape /\\./

highlight default link pinpointNote Comment
highlight default link pinpointSeparator Title
highlight default link pinpointSetting Type
highlight default link pinpointTag Statement
highlight default link pinpointEscape SpecialChar
let b:current_syntax = 'pinpoint'
